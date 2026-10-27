//! WuwaTappo (鸣潮桌宠) macOS 移植壳
//!
//! 逆向自 WuwaTappo.exe 1.0.1（Tauri 2.10.3）：
//! - 前端 dist 完整复用（.tmpfiles/extract/frontend）
//! - 自定义命令按官方签名 1:1 重建
//! - 托盘/面板/Toast 窗口模型照官方架构
#![allow(clippy::needless_return)]

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write as IoWrite;
use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, ContextMenu, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_updater::UpdaterExt;

mod input;

/// 桌宠缩放档位（前端 PET_SCALE_MIN=50 / MAX=200 / DEFAULT=100）
const PET_SCALE_STEPS: [f64; 7] = [50.0, 75.0, 100.0, 125.0, 150.0, 175.0, 200.0];
/// 窗口透明度档位（百分比）
const OPACITY_STEPS: [f64; 5] = [20.0, 40.0, 60.0, 80.0, 100.0];

/// 默认托盘文案（中文，sync_native_menu 会按语言覆盖）
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct MenuLabels {
    #[serde(default = "d_open_bag")]
    pub open_bag: String,
    #[serde(default = "d_open_atlas")]
    pub open_atlas: String,
    #[serde(default = "d_open_exchange")]
    pub open_exchange: String,
    #[serde(default = "d_open_room")]
    pub open_room: String,
    #[serde(default = "d_hide_pet")]
    pub hide_pet: String,
    #[serde(default = "d_show_pet")]
    pub show_pet: String,
    #[serde(default = "d_open_settings")]
    pub tray_open_settings: String,
    #[serde(default = "d_open_settings")]
    pub dock_settings: String,
    #[serde(default = "d_window_size")]
    pub window_size: String,
    #[serde(default = "d_window_passthrough")]
    pub window_passthrough: String,
    #[serde(default = "d_window_opacity")]
    pub window_opacity: String,
    #[serde(default = "d_restart")]
    pub restart: String,
    #[serde(default = "d_quit")]
    pub quit: String,
    #[serde(default = "d_tooltip")]
    pub tray_tooltip: String,
}
fn d_open_bag() -> String { "打开背包".into() }
fn d_open_atlas() -> String { "图鉴".into() }
fn d_open_exchange() -> String { "兑换".into() }
fn d_open_room() -> String { "联机房间".into() }
fn d_hide_pet() -> String { "隐藏桌宠".into() }
fn d_show_pet() -> String { "显示桌宠".into() }
fn d_open_settings() -> String { "设置".into() }
fn d_window_size() -> String { "桌宠大小".into() }
fn d_window_passthrough() -> String { "点击穿透".into() }
fn d_window_opacity() -> String { "不透明度".into() }
fn d_restart() -> String { "重启应用".into() }
fn d_quit() -> String { "退出".into() }
fn d_tooltip() -> String { "WuwaTappo".into() }

/// 托盘状态（sync_native_menu 持续同步）
#[derive(Debug, Clone, Default)]
pub struct TrayState {
    labels: MenuLabels,
    pet_visible: bool,
    pet_scale: f64,
    window_opacity: f64,
    window_passthrough: bool,
}
pub struct SharedTrayState(pub Mutex<TrayState>);

// ---------------------------------------------------------------------------
// 自定义命令（官方 7 个，签名与返回结构按逆向结果 1:1）
// ---------------------------------------------------------------------------

/// 更新检查：官方实现 = 用前端传入的 latest.json 端点跑 updater 流程。
/// 服务器 platforms 目前只有 windows-x86_64，darwin 目标缺失时按"无更新"处理。
#[tauri::command]
async fn check_app_update<R: Runtime>(
    app: AppHandle<R>,
    endpoint: String,
) -> Result<Option<serde_json::Value>, String> {
    println!("[cmd] check_app_update");
    let url = tauri::Url::parse(&endpoint).map_err(|e| e.to_string())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    let update = match updater.check().await {
        Ok(u) => u,
        // darwin 平台条目缺失（TargetsNotFound）等 → 视为无更新
        Err(_) => return Ok(None),
    };

    let Some(update) = update else { return Ok(None) };
    let date: Option<String> = None; // RFC3339 格式化留给后续（服务器暂无 darwin 更新）
    let rid = app.resources_table().add(update);
    Ok(Some(serde_json::json!({
        "rid": rid,
        "currentVersion": app.package_info().version.to_string(),
        "version": "0",
        "date": date,
        "body": Option::<String>::None,
        "rawJson": serde_json::json!({}),
    })))
}

/// 遥测系统信息（kurodata 埋点用）
#[tauri::command]
fn get_track_os_info() -> serde_json::Value {
    println!("[cmd] get_track_os_info");
    serde_json::json!({
        "os": std::env::consts::OS,
        "osVersion": "macOS",
    })
}

/// 强更锁（Windows 安装器更新期间锁定；macOS 恒为 false）
/// 注意：前端命令名带 d（isForceUpdateLocked → invoke("is_force_update_locked")）
#[tauri::command]
fn is_force_update_locked() -> bool {
    println!("[cmd] is_force_update_locked");
    false
}

/// 维护公告：emit 给前端渲染（官方为独立窗口，前端有 maintenance-notice-open 处理）
#[tauri::command]
fn open_maintenance_notice<R: Runtime>(app: AppHandle<R>, payload: Option<serde_json::Value>) {
    println!("[cmd] open_maintenance_notice");
    if let Err(e) = app.emit("maintenance-notice-open", payload) {
        log::warn!("[maintenance-notice] emit failed: {e}");
    }
}

/// Toast 覆盖层：创建/显示 label="toast" 的透明置顶小窗，前端跨窗口事件渲染内容
#[tauri::command]
async fn show_toast_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    println!("[cmd] show_toast_overlay");
    const TOAST_LABEL: &str = "toast";
    if let Some(win) = app.get_webview_window(TOAST_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let (w, h) = (420.0, 120.0);
    let mut b = WebviewWindowBuilder::new(
        &app,
        TOAST_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("WuwaTappo Toast")
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .shadow(false)
    .inner_size(w, h);
    if let Ok(monitor) = app.primary_monitor() {
        if let Some(m) = monitor {
            let size = m.size();
            let scale = m.scale_factor();
            let x = (size.width as f64 / scale - w) / 2.0;
            let y = 48.0;
            b = b.position(x, y);
        }
    }
    b.build().map_err(|e| e.to_string())?;
    Ok(())
}

/// 同步原生（托盘）菜单：按前端传入的多语言文案 + 当前设置重建菜单
#[tauri::command]
fn sync_native_menu<R: Runtime>(
    app: AppHandle<R>,
    labels: MenuLabels,
    pet_dock_visible: Option<bool>,
    pet_scale: Option<f64>,
    window_opacity: Option<f64>,
    window_passthrough: Option<bool>,
) {
    println!("[cmd] sync_native_menu");
    let state = TrayState {
        labels,
        pet_visible: pet_dock_visible.unwrap_or(true),
        pet_scale: pet_scale.unwrap_or(100.0),
        window_opacity: window_opacity.unwrap_or(100.0),
        window_passthrough: window_passthrough.unwrap_or(false),
    };
    if let Some(shared) = app.try_state::<SharedTrayState>() {
        *shared.0.lock().unwrap() = state.clone();
    }
    let _ = rebuild_tray_menu(&app, &state);
}

/// 应用日志：$APPLOCALDATA/logs/app-YYYYMMDD.log
#[tauri::command]
fn write_app_log<R: Runtime>(app: AppHandle<R>, level: String, message: String) -> Result<(), String> {
    println!("[cmd] write_app_log");
    use tauri::path::BaseDirectory;
    let dir = app
        .path()
        .resolve("logs", BaseDirectory::AppLocalData)
        .map_err(|e| e.to_string())?;
    create_dir_all(&dir).map_err(|e| e.to_string())?;
    let date = chrono_date();
    let path = dir.join(format!("app-{date}.log"));
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let line = format!("[{}] [{}] {}\n", chrono_time(), level, message);
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

fn chrono_date() -> String {
    // 简易本地日期（避免引入 chrono；精度到分钟即可）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    // 1970-01-01 起
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}{m:02}{d:02}")
}
fn chrono_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, mi, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{mi:02}:{s:02}")
}
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// 桌宠主窗口管理命令（11 个，藏于前端 kt() 封装之后，阶段 2 挖掘补全）
// macOS 策略：Win32 区域命中/鼠标吞噬等专属技巧按语义降级映射
// ---------------------------------------------------------------------------

/// 区域穿透与命中状态同步。
/// 官方全屏 shell（MAIN_PET_FULLSCREEN_SHELL=true）：main 窗口 WebView 恒穿透
/// （官方日志 "[passthrough] force_cursor_ignore=true (WebView always ignores)"），
/// 桌宠的点击/拖拽/右键菜单由前端基于 mouse-move / mouse-button 全局事件合成
/// （elementFromPoint + click / beginDockPointerAt 手势 / popup 命令）。
/// macOS 照搬恒穿透；同时把前端上报的区域存入 SharedMainHitState，
/// 供 input.rs 的 mouse-button 路由做 per-region 命中过滤
/// （官方 Windows 的 WM_NCHITTEST 等价：只有命中桌宠区域的鼠标事件才进桥）。
#[tauri::command]
fn set_main_hit_region<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] set_main_hit_region");
    #[derive(serde::Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct P {
        #[serde(default)]
        regions: Option<serde_json::Value>,
        #[serde(default)]
        input_regions: Option<serde_json::Value>,
    }
    let p: P = serde_json::from_value(payload)
        .map_err(|e| format!("set_main_hit_region payload: {e}"))?;
    let visual = input::parse_regions(p.regions.as_ref());
    let inputs = input::parse_regions(p.input_regions.as_ref());
    *app.state::<input::SharedMainHitState>().0.lock().unwrap() = input::MainHitState {
        input_regions: inputs,
        visual_regions: visual,
    };
    if let Some(win) = app.get_webview_window("main") {
        win.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 强制全窗光标穿透（全屏拖拽冻结恢复等场景）
#[tauri::command]
fn set_main_force_cursor_ignore<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    println!("[cmd] set_main_force_cursor_ignore");
    if let Some(win) = app.get_webview_window("main") {
        win.set_ignore_cursor_events(enabled).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 鼠标吞噬（Win32 专属：穿透切换瞬间吞掉鼠标消息防闪烁）；macOS 无此问题，空实现保协议
#[tauri::command]
fn set_main_swallow_mouse<R: Runtime>(_app: AppHandle<R>, enabled: bool) {
    println!("[cmd] set_main_swallow_mouse");
    log::debug!("[swallow-mouse] enabled={enabled} (no-op on macOS)");
}

/// 任务栏图标显隐（macOS: NSApp activation policy Regular↔Accessory，即 Dock 图标）
#[tauri::command]
fn set_main_skip_taskbar<R: Runtime>(app: AppHandle<R>, skip: bool) -> Result<(), String> {
    println!("[cmd] set_main_skip_taskbar");
    if let Some(win) = app.get_webview_window("main") {
        win.set_skip_taskbar(skip).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 主窗口内容边界：前端按 DOM 实测内容区域同步窗口几何（resize/拖拽后）
#[tauri::command]
fn set_main_window_content_bounds<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] set_main_window_content_bounds");
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct P {
        width: f64,
        height: f64,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        #[allow(dead_code)]
        skip_passthrough_refresh: Option<bool>,
    }
    let p: P = serde_json::from_value(payload)
        .map_err(|e| format!("content_bounds payload: {e}"))?;
    println!(
        "[content-bounds] w={} h={} x={:?} y={:?}",
        p.width, p.height, p.x, p.y
    );
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    if let (Some(x), Some(y)) = (p.x, p.y) {
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
    win.set_size(tauri::LogicalSize::new(p.width, p.height))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 覆盖光标（Windows 宠物 hover 换手型光标）；前端仅有 kind:"default" 恢复调用，macOS 系统自管
#[tauri::command]
fn set_overlay_cursor<R: Runtime>(_app: AppHandle<R>, kind: String) {
    println!("[cmd] set_overlay_cursor");
    log::debug!("[overlay-cursor] kind={kind} (no-op on macOS)");
}

/// 同步主窗口到鼠标所在显示器（多屏拖拽后校正坐标/缩放）
#[tauri::command]
fn sync_main_window_to_current_monitor<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    println!("[cmd] sync_main_window_to_current_monitor");
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    let monitor = win
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return Ok(()) };
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;
    let mp = m.position();
    let ms = m.size();
    let scale = m.scale_factor();
    // 逻辑坐标换算后 clamp 进显示器范围，避免窗口滞留在旧屏坐标
    let (mx, my) = (mp.x as f64 / scale, mp.y as f64 / scale);
    let (mw, mh) = (ms.width as f64 / scale, ms.height as f64 / scale);
    let (wx, wy) = (pos.x as f64 / scale, pos.y as f64 / scale);
    let (ww, wh) = (size.width as f64 / scale, size.height as f64 / scale);
    let cx = wx.clamp(mx - ww + 40.0, mx + mw - 40.0);
    let cy = wy.clamp(my - wh + 40.0, my + mh - 40.0);
    if (cx - wx).abs() > 0.5 || (cy - wy).abs() > 0.5 {
        win.set_position(tauri::LogicalPosition::new(cx, cy))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 宠物底座右键菜单：官方 Windows 原生 HMENU 坐标弹出；
/// macOS 用 Tauri ContextMenu::popup（muda 以当前鼠标位置弹出 NSMenu）
#[tauri::command]
fn popup_main_dock_context_menu<R: Runtime>(
    app: AppHandle<R>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    println!("[cmd] popup_main_dock_context_menu");
    let _ = (x, y); // 官方传全局坐标；popup 自动跟随鼠标位置
    let st = app
        .state::<SharedTrayState>()
        .0
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let menu = build_dock_menu(&app, &st).map_err(|e| e.to_string())?;
    if let Some(win) = app.get_webview_window("main") {
        let window = win.as_ref().window();
        menu.popup(window).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 启动诊断（前端 4s 超时容忍缺失，逐字段 fallback 上报遥测）
#[tauri::command]
fn get_boot_diagnostics() -> serde_json::Value {
    println!("[cmd] get_boot_diagnostics");
    serde_json::json!({
        "system": {
            "os": std::env::consts::OS,
            "family": std::env::consts::FAMILY,
            "arch": std::env::consts::ARCH,
            "kernel": "darwin",
        },
        "runtime": { "webview_version": "wkwebview" },
        "cpu": { "arch": std::env::consts::ARCH },
        "memory": null,
        "gpus": null,
    })
}

/// 是否 devtools 快捷键构建（F9 等；仅内部构建开启）
#[tauri::command]
fn is_devtools_hotkeys_build() -> bool {
    println!("[cmd] is_devtools_hotkeys_build");
    false
}

/// 是否内部测试构建
#[tauri::command]
fn is_internal_testing_build() -> bool {
    println!("[cmd] is_internal_testing_build");
    false
}

// ---------------------------------------------------------------------------
// 窗口布局/显示器切换（switchDisplayMonitor chunk，7 个）
// ---------------------------------------------------------------------------

/// 应用主窗口位置策略：alwaysOnTop 立即生效；applyPosition=true 时同时落位
#[tauri::command]
fn apply_main_window_position<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] apply_main_window_position");
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct P {
        always_on_top: Option<bool>,
        // position 可能是字符串预设（"bottomRight"）或 {x,y} 坐标
        #[serde(default)]
        position: Option<serde_json::Value>,
        apply_position: Option<bool>,
    }
    let p: P = serde_json::from_value(payload).map_err(|e| e.to_string())?;
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    if let Some(top) = p.always_on_top {
        win.set_always_on_top(top).map_err(|e| e.to_string())?;
    }
    if p.apply_position.unwrap_or(false) {
        if let Some(pos) = &p.position {
            let x = pos.get("x").and_then(|v| v.as_f64());
            let y = pos.get("y").and_then(|v| v.as_f64());
            if let (Some(x), Some(y)) = (x, y) {
                let _ = win.set_position(tauri::LogicalPosition::new(x, y));
            }
            // 字符串预设由前端换算，此处无需处理
        }
    }
    Ok(())
}

/// Windows 全屏 shell 模式（穿透层接管全屏）。
/// 有意返回 Err：让前端走官方 fallback（setPosition/setSize + sync monitor），macOS 语义更正确。
#[tauri::command]
fn set_main_window_fullscreen_shell<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    println!("[cmd] set_main_window_fullscreen_shell");
    Err("fullscreen shell is windows-only; use window api fallback".into())
}

/// 显示器切换：把主窗口移动到目标显示器坐标
#[tauri::command]
fn set_main_window_monitor_bounds<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] set_main_window_monitor_bounds");
    #[derive(serde::Deserialize, Debug)]
    struct P {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }
    let p: P = serde_json::from_value(payload).map_err(|e| e.to_string())?;
    println!("[monitor-bounds] {p:?}");
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    win.set_position(tauri::LogicalPosition::new(p.x, p.y))
        .map_err(|e| e.to_string())?;
    win.set_size(tauri::LogicalSize::new(p.width, p.height))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 桌面安全区（Windows 任务栏避让）；macOS 桌宠窗口不与 Dock 重叠，返回零 insets
#[tauri::command]
fn get_main_desktop_safe_area() -> serde_json::Value {
    println!("[cmd] get_main_desktop_safe_area");
    serde_json::json!({
        "insets": {"left": 0.0, "top": 0.0, "right": 0.0, "bottom": 0.0},
        "autoHideEdge": null,
    })
}

/// 任务栏 insets（Windows）；macOS 恒零
#[tauri::command]
fn get_main_taskbar_insets() -> serde_json::Value {
    println!("[cmd] get_main_taskbar_insets");
    serde_json::json!({"left": 0.0, "top": 0.0, "right": 0.0, "bottom": 0.0})
}

/// 显示器切换收尾（Windows 恢复输入劫持）；macOS no-op
#[tauri::command]
fn finish_main_window_monitor_switch<R: Runtime>(_app: AppHandle<R>) {
    println!("[cmd] finish_main_window_monitor_switch");
    log::debug!("[monitor-switch] finish (no-op on macOS)");
}

/// 辅助窗口（toast 等）跟随主窗口显示器
#[tauri::command]
fn sync_aux_windows_to_main_monitor<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] sync_aux_windows_to_main_monitor");
    let Some(main) = app.get_webview_window("main") else { return };
    let Ok(Some(mon)) = main.current_monitor() else { return };
    let Ok(pos) = main.outer_position() else { return };
    if let Some(toast) = app.get_webview_window("toast") {
        // toast 保持屏幕顶部居中——按新显示器重算
        let scale = mon.scale_factor();
        let w = toast.outer_size().map(|s| s.width as f64 / scale).unwrap_or(420.0);
        let mx = mon.position().x as f64 / scale;
        let mw = mon.size().width as f64 / scale;
        let _ = toast.set_position(tauri::LogicalPosition::new(mx + (mw - w) / 2.0, 48.0));
    }
    let _ = pos;
}

// ---------------------------------------------------------------------------
// update / 维护公告窗口（index-BE0i_XCi.js，4 个）
// ---------------------------------------------------------------------------

/// 上次检查到的更新详情；macOS 无官方更新通道，恒 null（前端按无更新渲染）
#[tauri::command]
fn get_app_update_detail() -> Option<serde_json::Value> {
    println!("[cmd] get_app_update_detail");
    None
}

/// 维护公告内容；无服务端公告，恒 null
#[tauri::command]
fn get_maintenance_notice_payload() -> Option<serde_json::Value> {
    println!("[cmd] get_maintenance_notice_payload");
    None
}

/// 是否有维护公告待展示
#[tauri::command]
fn has_maintenance_notice() -> bool {
    println!("[cmd] has_maintenance_notice");
    false
}

/// 维护公告就绪后隐藏主窗口
#[tauri::command]
fn hide_main_after_maintenance_notice_ready<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] hide_main_after_maintenance_notice_ready");
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

// ---------------------------------------------------------------------------
// 辅助窗口管理（modalWindows/appAutostart chunk，12 个）
// ---------------------------------------------------------------------------

/// 通用辅助窗口创建（hash 路由与官方一致：#/panel /#/login /#/update /#/about /#/oss-notice）
fn create_aux_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    route: &str,
    title: &str,
    w: f64,
    h: f64,
    resizable: bool,
) -> tauri::Result<()> {
    create_aux_window_min(app, label, route, title, w, h, resizable, 280.0, 200.0)
}

/// 同上，允许指定窗口最小尺寸（panel 内容较大，min 需要更高下限）
fn create_aux_window_min<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    route: &str,
    title: &str,
    w: f64,
    h: f64,
    resizable: bool,
    min_w: f64,
    min_h: f64,
) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html#{route}").into()),
    )
    .title(title)
    .inner_size(w, h)
    .min_inner_size(min_w, min_h)
    .decorations(false)
    .transparent(true)
    .resizable(resizable)
    .shadow(true)
    .build()?;
    Ok(())
}

/// 打开/聚焦 panel 窗口（前端 dock-open-panel 链路的 Rust 落点）。
/// 初始 1040x720：panel 是自绘 resize handle 的无边框窗口，880x640 时
/// 内容（含右上角关闭按钮）显示不全，需手动拉大才完整。
/// min 与初始一致：panel 前端 UI 非响应式（小窗口内容被裁切），
/// 限制不可缩小于 1040x720，保证任意合法窗口尺寸下 UI 完整显示；
/// 折叠功能是 panel 内部 CSS 类切换（module-panel--collapsed），
/// 不依赖窗口缩小，不受 min 限制影响。
const PANEL_SIZE: (f64, f64, f64, f64) = (1040.0, 720.0, 1040.0, 720.0);

fn open_panel<R: Runtime>(app: &AppHandle<R>, tab: &str) -> tauri::Result<()> {
    let (w, h, mw, mh) = PANEL_SIZE;
    create_aux_window_min(app, "panel", "/panel", "WuwaTappo", w, h, true, mw, mh)?;
    let _ = app.emit_to("panel", "panel-open-tab", tab);
    Ok(())
}

/// 前端入口：打开 panel 窗口（payload.tab 为空时仅聚焦）
#[tauri::command]
fn open_panel_window<R: Runtime>(
    app: AppHandle<R>,
    payload: Option<serde_json::Value>,
) -> Result<(), String> {
    println!("[cmd] open_panel_window");
    let tab = payload
        .as_ref()
        .and_then(|p| p.get("tab"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (w, h, mw, mh) = PANEL_SIZE;
    match tab {
        Some(t) if !t.is_empty() => open_panel(&app, &t),
        _ => create_aux_window_min(&app, "panel", "/panel", "WuwaTappo", w, h, true, mw, mh),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_panel_window<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] close_panel_window");
    if let Some(win) = app.get_webview_window("panel") {
        let _ = win.close();
    }
}

/// panel 浮到桌宠之上（宠物会遮挡面板时的层级修正）
#[tauri::command]
fn raise_panel_window_above_pets<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] raise_panel_window_above_pets");
    if let Some(win) = app.get_webview_window("panel") {
        let _ = win.set_always_on_top(true);
        let _ = win.set_focus();
    }
}

/// panel 折叠态（官方由 Rust 调整窗口几何；前端亦自理尺寸，macOS 记录状态即可）
#[tauri::command]
fn set_panel_window_collapsed<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] set_panel_window_collapsed");
    let collapsed = payload
        .get("collapsed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    log::debug!("[panel] collapsed={collapsed}");
    let _ = app;
    Ok(())
}

#[tauri::command]
fn open_login_window<R: Runtime>(
    app: AppHandle<R>,
    intent: Option<String>,
    compact: Option<bool>,
) -> Result<(), String> {
    println!("[cmd] open_login_window");
    let _ = (intent, compact);
    create_aux_window(&app, "login", "/login", "WuwaTappo", 400.0, 620.0, false)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_login_window<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] close_login_window");
    if let Some(win) = app.get_webview_window("login") {
        let _ = win.close();
    }
}

#[tauri::command]
fn resize_login_window_to_content<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> Result<(), String> {
    println!("[cmd] resize_login_window_to_content");
    let w = payload.get("width").and_then(|v| v.as_f64()).unwrap_or(400.0);
    let h = payload.get("height").and_then(|v| v.as_f64()).unwrap_or(620.0);
    if let Some(win) = app.get_webview_window("login") {
        win.set_size(tauri::LogicalSize::new(w, h))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_update_window<R: Runtime>(
    app: AppHandle<R>,
    detail: Option<serde_json::Value>,
) -> Result<(), String> {
    println!("[cmd] open_update_window");
    create_aux_window(&app, "update", "/update", "WuwaTappo", 420.0, 560.0, false)
        .map_err(|e| e.to_string())?;
    // 官方在窗口就绪后下发 detail；前端亦可经 get_app_update_detail 拉取
    if let Some(d) = detail {
        let _ = app.emit_to("update", "update-detail", d);
    }
    Ok(())
}

#[tauri::command]
fn close_update_window<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] close_update_window");
    if let Some(win) = app.get_webview_window("update") {
        let _ = win.close();
    }
}

#[tauri::command]
fn open_about_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    println!("[cmd] open_about_window");
    create_aux_window(&app, "about", "/about", "WuwaTappo", 420.0, 520.0, false)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_about_window<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] close_about_window");
    if let Some(win) = app.get_webview_window("about") {
        let _ = win.close();
    }
}

#[tauri::command]
fn open_oss_notice_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    println!("[cmd] open_oss_notice_window");
    create_aux_window(&app, "oss-notice", "/oss-notice", "WuwaTappo", 460.0, 600.0, true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_oss_notice_window<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] close_oss_notice_window");
    if let Some(win) = app.get_webview_window("oss-notice") {
        let _ = win.close();
    }
}

// ---------------------------------------------------------------------------
// 自启动（Windows 提权版；macOS 直接映射 LaunchAgent）
// ---------------------------------------------------------------------------

#[tauri::command]
fn is_elevated_autostart_enabled<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    println!("[cmd] is_elevated_autostart_enabled");
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_elevated_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    println!("[cmd] set_elevated_autostart");
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())
    } else {
        autolaunch.disable().map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// 前端本地计数存储（useLogin chunk，Rust 侧 JSON 落盘）
// ---------------------------------------------------------------------------

fn store_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    use tauri::path::BaseDirectory;
    let dir = app
        .path()
        .resolve("native-store", BaseDirectory::AppLocalData)
        .map_err(|e| e.to_string())?;
    create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("store.json"))
}

fn store_read<R: Runtime>(app: &AppHandle<R>) -> serde_json::Value {
    match store_path(app) {
        Ok(p) => std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

fn store_write<R: Runtime>(app: &AppHandle<R>, v: &serde_json::Value) {
    if let Ok(p) = store_path(app) {
        if let Ok(s) = serde_json::to_string_pretty(v) {
            let _ = std::fs::write(p, s);
        }
    }
}

/// 返回裸数字：前端 `parseSafeClickCount` 直接 `Number(payload)`，
/// 返回对象会被解析成 NaN→0，导致每次启动计数归零。
#[tauri::command]
fn load_key_press_count<R: Runtime>(
    app: AppHandle<R>,
    environment: String,
) -> i64 {
    println!("[cmd] load_key_press_count");
    let store = store_read(&app);
    let entry = store
        .get("key_press")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let env = entry
        .get("environment")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if env != environment {
        return 0;
    }
    entry.get("count").and_then(|v| v.as_i64()).unwrap_or(0)
}

#[tauri::command]
fn save_key_press_count<R: Runtime>(
    app: AppHandle<R>,
    count: i64,
    environment: String,
) {
    println!("[cmd] save_key_press_count");
    let mut store = store_read(&app);
    store["key_press"] = serde_json::json!({"count": count, "environment": environment});
    store_write(&app, &store);
}

#[tauri::command]
fn load_cumulative_click_count<R: Runtime>(
    app: AppHandle<R>,
    environment: String,
) -> serde_json::Value {
    println!("[cmd] load_cumulative_click_count");
    let store = store_read(&app);
    let entry = store
        .get("cumulative_click")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let env = entry
        .get("environment")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if env != environment {
        return serde_json::json!({"count": 0, "cuid": "", "environment": ""});
    }
    serde_json::json!({
        "count": entry.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
        "cuid": entry.get("cuid").and_then(|v| v.as_str()).unwrap_or(""),
        "environment": env,
    })
}

#[tauri::command]
fn save_cumulative_click_count<R: Runtime>(
    app: AppHandle<R>,
    count: i64,
    cuid: String,
    environment: String,
) {
    println!("[cmd] save_cumulative_click_count");
    let mut store = store_read(&app);
    store["cumulative_click"] =
        serde_json::json!({"count": count, "cuid": cuid, "environment": environment});
    store_write(&app, &store);
}

// ---------------------------------------------------------------------------
// 输入桥就绪握手（input.rs 事件转发门控）
// ---------------------------------------------------------------------------

/// 前端完成 key-press/mouse 监听注册后握手；tap 在握手前不向 WebView emit，
/// 避免高频事件轰炸未就绪的 IPC 通道（曾导致监听注册竞态失败）。
pub struct InputBridgeReady(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

#[tauri::command]
fn notify_input_bridge_ready<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] notify_input_bridge_ready");
    input::mark_input_bridge_ready(&app);
}

// ---------------------------------------------------------------------------
// 退出（quitApp chunk）
// ---------------------------------------------------------------------------

/// 前端完成 socket 断开与计数落盘后的最终退出
#[tauri::command]
fn exit_app<R: Runtime>(app: AppHandle<R>) {
    println!("[cmd] exit_app");
    app.exit(0);
}

// ---------------------------------------------------------------------------
// 托盘
// ---------------------------------------------------------------------------

fn rebuild_tray_menu<R: Runtime>(app: &AppHandle<R>, st: &TrayState) -> tauri::Result<()> {
    let menu = build_dock_menu(app, st)?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu))?;
        let _ = tray.set_tooltip(Some(st.labels.tray_tooltip.clone()));
    }
    Ok(())
}

/// 构建托盘/右键共用的桌宠菜单（菜单事件走统一的 handle_menu_event）
pub fn build_dock_menu<R: Runtime>(
    app: &AppHandle<R>,
    st: &TrayState,
) -> tauri::Result<Menu<R>> {
    let l = &st.labels;
    let open_bag = MenuItem::with_id(app, "open_bag", &l.open_bag, true, Option::<&str>::None)?;
    let open_atlas = MenuItem::with_id(app, "open_atlas", &l.open_atlas, true, Option::<&str>::None)?;
    let open_exchange = MenuItem::with_id(app, "open_exchange", &l.open_exchange, true, Option::<&str>::None)?;
    let open_room = MenuItem::with_id(app, "open_room", &l.open_room, true, Option::<&str>::None)?;
    let toggle_pet = MenuItem::with_id(
        app,
        "toggle_pet",
        if st.pet_visible { &l.hide_pet } else { &l.show_pet },
        true,
        Option::<&str>::None,
    )?;
    let open_settings = MenuItem::with_id(app, "open_settings", &l.tray_open_settings, true, Option::<&str>::None)?;

    // 缩放子菜单（单选语义，勾选当前档）
    let mut scale_items = Vec::new();
    for step in PET_SCALE_STEPS {
        let id = format!("pet_scale_{step}");
        let label = if step == 100.0 { "100%".to_string() } else { format!("{}%", step as i64) };
        scale_items.push(CheckMenuItem::with_id(app, id, label, true, (st.pet_scale - step).abs() < 0.5, Option::<&str>::None)?);
    }
    let scale_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = scale_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<R>).collect();
    let scale_submenu = Submenu::with_id_and_items(app, "pet_scale", &l.window_size, true, &scale_refs)?;

    // 透明度子菜单
    let mut opacity_items = Vec::new();
    for step in OPACITY_STEPS {
        let id = format!("opacity_{step}");
        opacity_items.push(CheckMenuItem::with_id(app, id, format!("{}%", step as i64), true, (st.window_opacity - step).abs() < 0.5, Option::<&str>::None)?);
    }
    let opacity_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = opacity_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<R>).collect();
    let opacity_submenu = Submenu::with_id_and_items(app, "opacity", &l.window_opacity, true, &opacity_refs)?;

    let passthrough = CheckMenuItem::with_id(app, "passthrough", &l.window_passthrough, true, st.window_passthrough, Option::<&str>::None)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let restart = MenuItem::with_id(app, "tray_restart", &l.restart, true, Option::<&str>::None)?;
    let quit = MenuItem::with_id(app, "tray_quit", &l.quit, true, Option::<&str>::None)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_bag, &open_atlas, &open_exchange, &open_room, &sep1,
            &toggle_pet, &scale_submenu, &opacity_submenu, &passthrough, &sep2,
            &open_settings, &restart, &quit,
        ],
    )?;

    Ok(menu)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let st = app
        .try_state::<SharedTrayState>()
        .map(|s| s.0.lock().unwrap().clone())
        .unwrap_or_default();

    match id {
        // 打开面板：官方事件名 dock-open-panel，payload 为 tab 字符串，前端走完整链路
        "open_bag" | "open_atlas" | "open_exchange" | "open_room" | "open_settings" => {
            let tab = id.trim_start_matches("open_");
            let _ = app.emit("dock-open-panel", tab);
        }
        // 显隐/穿透为 toggle 语义，前端自行处理状态与持久化
        "toggle_pet" => {
            let _ = app.emit("tray-toggle-pet-visibility", ());
        }
        "passthrough" => {
            let _ = app.emit("tray-toggle-window-passthrough", ());
        }
        // 缩放/透明度：payload 为数字
        id if id.starts_with("pet_scale_") => {
            let scale: f64 = id["pet_scale_".len()..].parse().unwrap_or(100.0);
            let _ = app.emit("tray-set-pet-scale", scale);
            let mut st2 = st;
            st2.pet_scale = scale;
            if let Some(shared) = app.try_state::<SharedTrayState>() {
                *shared.0.lock().unwrap() = st2.clone();
            }
            let _ = rebuild_tray_menu(app, &st2);
        }
        id if id.starts_with("opacity_") => {
            let op: f64 = id["opacity_".len()..].parse().unwrap_or(100.0);
            let _ = app.emit("tray-set-window-opacity", op);
            let mut st2 = st;
            st2.window_opacity = op;
            if let Some(shared) = app.try_state::<SharedTrayState>() {
                *shared.0.lock().unwrap() = st2.clone();
            }
            let _ = rebuild_tray_menu(app, &st2);
        }
        // 重启/退出：前端清理（socket 断开、计数落盘）后自行调 exit_app / relaunch
        "tray_restart" => {
            let _ = app.emit("tray-restart", ());
        }
        "tray_quit" => {
            let _ = app.emit("tray-quit", ());
            // 兜底：前端清理失败时 3 秒后强制退出
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                app2.exit(0);
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 应用入口
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // updater 状态必须由插件注册（check_app_update 的 updater_builder 依赖它，
        // 缺失会 panic: state() called before manage() for UpdaterState）
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            use tauri::tray::TrayIconBuilder;
            app.manage(SharedTrayState(Mutex::new(TrayState::default())));
            app.manage(input::SharedMainHitState(Mutex::new(input::MainHitState::default())));
            app.manage(InputBridgeReady(std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            )));

            // 托盘
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        // 官方语义：dock-open-panel → 前端（含强更锁判断）→ open_panel_window
                        let _ = tray.app_handle().emit("dock-open-panel", "bag");
                    }
                })
                .build(app)?;

            // 初始托盘菜单
            let st = app.state::<SharedTrayState>().0.lock().unwrap().clone();
            rebuild_tray_menu(app.handle(), &st)?;

            // main 窗口初始设置：多工作区可见（桌宠惯例）
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_visible_on_all_workspaces(true);
            }

            // 全局键鼠监听（key-press/key-release/mouse-move 事件源；
            // 辅助功能授权后自动生效，未授权时仅日志提示不影响启动）
            input::start(app.handle().clone());
            Ok(())
        })
        // WKWebView 默认右键菜单（Reload/Back 等）不符合桌面应用观感；
        // 官方前端仅对桌宠 dock 区域做 contextmenu preventDefault（$p），
        // 这里对所有窗口统一屏蔽。桌宠右键菜单由前端基于 mouse-button
        // 事件调 popup_main_dock_context_menu 弹原生菜单。
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = webview.eval(
                    "document.addEventListener('contextmenu', e => e.preventDefault(), { capture: true });",
                );
            }
        })
        .invoke_handler(tauri::generate_handler![
            check_app_update,
            get_track_os_info,
            is_force_update_locked,
            open_maintenance_notice,
            show_toast_overlay,
            sync_native_menu,
            write_app_log,
            // 桌宠主窗口管理（kt 封装挖掘补全）
            set_main_hit_region,
            set_main_force_cursor_ignore,
            set_main_swallow_mouse,
            set_main_skip_taskbar,
            set_main_window_content_bounds,
            set_overlay_cursor,
            sync_main_window_to_current_monitor,
            popup_main_dock_context_menu,
            get_boot_diagnostics,
            is_devtools_hotkeys_build,
            is_internal_testing_build,
            // 显示器/窗口布局
            apply_main_window_position,
            set_main_window_fullscreen_shell,
            set_main_window_monitor_bounds,
            get_main_desktop_safe_area,
            get_main_taskbar_insets,
            finish_main_window_monitor_switch,
            sync_aux_windows_to_main_monitor,
            // update / 维护公告
            get_app_update_detail,
            get_maintenance_notice_payload,
            has_maintenance_notice,
            hide_main_after_maintenance_notice_ready,
            // 辅助窗口管理
            open_panel_window,
            close_panel_window,
            raise_panel_window_above_pets,
            set_panel_window_collapsed,
            open_login_window,
            close_login_window,
            resize_login_window_to_content,
            open_update_window,
            close_update_window,
            open_about_window,
            close_about_window,
            open_oss_notice_window,
            close_oss_notice_window,
            // 自启动 / 存储 / 退出
            is_elevated_autostart_enabled,
            set_elevated_autostart,
            load_key_press_count,
            save_key_press_count,
            load_cumulative_click_count,
            save_cumulative_click_count,
            notify_input_bridge_ready,
            exit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
