//! 全局输入监听（macOS）：CGEventTap → Tauri 事件桥。
//!
//! 事件协议（与前端 dist 对齐，`index-DmC0wUEw.js` 原生输入 POC）：
//! - `key-press` / `key-release`：payload 为 `KeyboardEvent.key` 风格字符串
//!   （字母大写单字符、空格为 `" "`、如 `"F5"`、`"Shift"`、`"ArrowUp"`）
//! - `mouse-move`：payload `{ "x": f64, "y": f64 }`，**全局物理像素**。
//!   前端按 `(x - window.innerPosition) / scaleFactor` 换算成窗口内 CSS 坐标
//!   （见 dist 中 `Ht()`），因此这里必须把 CG 逻辑点换算为物理像素。
//! - `mouse-button`：payload `{ "button": "left"|"right", "action": "press"|"release" }`。
//!
//! 官方全屏 shell 架构：main 窗口 WebView 恒穿透（ignoresMouseEvents），
//! 拖拽（beginDockPointerAt 手势）、按钮/宝箱点击（elementFromPoint + click()）、
//! 右键菜单（popup_main_dock_context_menu）全部由前端基于上面两个鼠标事件
//! 合成，Rust 侧不做命中测试。
//!
//! CGEventTap 挂法参考 ayangweb/BongoCat（Apache-2.0）
//! `crates/bongocat-platform/src/macos.rs`：session tail 的 tap 会丢失部分
//! FlagsChanged（右 Shift 释放），必须 `HIDEventTap + HeadInsertEventTap`；
//! 键码 → 键名映射亦按其 keycode→HID usage 表校准（小键盘区 0x52..0x5C，
//! 0x4F/0x50 为 F18/F19）。

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventMask, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use tauri::{AppHandle, Emitter, Manager};

// ---------------------------------------------------------------------------
// hit region 状态（前端 set_main_hit_region 同步的窗口内 CSS 逻辑坐标）
// ---------------------------------------------------------------------------

/// 窗口内逻辑坐标矩形（与前端 CSS px 一致）
#[derive(Debug, Clone, Copy, Default)]
pub struct RegionRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RegionRect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[derive(Default)]
pub struct MainHitState {
    /// 可交互区域（引导按钮、宝箱、panel 控件等）
    pub input_regions: Vec<RegionRect>,
    /// 实体可视区域（桌宠身体、dock 等）
    pub visual_regions: Vec<RegionRect>,
}

pub struct SharedMainHitState(pub std::sync::Mutex<MainHitState>);

/// 宽松解析前端 regions 数组：字段值可能是数字或字符串数字，含 href 等附加字段忽略。
/// 横坐标兼容 x / left、纵坐标兼容 y / top（实测前端上报的是 left/top——
/// 2026-10-28 诊断日志实锤：只认 x/y 时 parsed 恒为 0，桌宠点击/拖拽全灭）
pub fn parse_regions(v: Option<&serde_json::Value>) -> Vec<RegionRect> {
    let mut out = Vec::new();
    if let Some(arr) = v.and_then(|x| x.as_array()) {
        for item in arr {
            let f = |k: &str| {
                item.get(k).and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
                })
            };
            let coord = |primary: &str, alt: &str| f(primary).or_else(|| f(alt));
            if let (Some(x), Some(y), Some(w), Some(h)) = (
                coord("x", "left"),
                coord("y", "top"),
                f("width"),
                f("height"),
            ) {
                out.push(RegionRect { x, y, w, h });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// mouse-button 路由（官方 per-region 语义的 macOS 等价）
// ---------------------------------------------------------------------------

/// main 窗口 + 辅助窗口几何快照（全局逻辑坐标）。
///
/// 为什么需要缓存：CGEventTap 回调跑在独立线程，而 Tauri/tao 的窗口查询
/// （`get_webview_window` / `outer_position` / `is_visible`）必须在主线程。
/// 之前直接在 tap 线程查窗口 API，进程会不定时静默崩溃（用户实测进程反复消失）。
/// 现在命中测试只读这份缓存，由主线程命令刷新。
#[derive(Default, Clone)]
pub struct MainGeom {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// (label, x, y, w, h)：本进程其他可见窗口，点击落在其中时不算桌宠命中
    pub aux: Vec<(String, f64, f64, f64, f64)>,
    /// 前端通过 set_main_force_cursor_ignore 表达的"希望全窗穿透"期望
    pub front_force_ignore: bool,
    /// 当前指针是否在桌宠交互区域内（由 tap 线程写）
    pub pointer_in_pet: bool,
}

pub struct SharedMainGeom(pub std::sync::Mutex<MainGeom>);

/// 主线程刷新几何缓存（窗口创建/移动/显隐、前端上报区域时调用）。
/// 100ms 节流：前端每次点击都会上报 hit region，避免 IPC 线程被窗口查询拖慢。
pub fn refresh_main_geom<R: tauri::Runtime>(app: &AppHandle<R>) {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = now_ms();
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && now - last < 100 {
        return;
    }
    LAST.store(now, Ordering::Relaxed);
    // 保留上次的交互态字段（几何刷新不应重置指针/前端期望状态）
    let (front_force_ignore, pointer_in_pet) = app
        .state::<SharedMainGeom>()
        .0
        .lock()
        .map(|g| (g.front_force_ignore, g.pointer_in_pet))
        .unwrap_or((true, false));
    let mut g = MainGeom {
        front_force_ignore,
        pointer_in_pet,
        ..Default::default()
    };
    let mut aux = Vec::new();
    for (label, win) in app.webview_windows() {
        let visible = win.is_visible().unwrap_or(false);
        let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size()) else {
            continue;
        };
        let (x, y, w, h) = (p.x as f64, p.y as f64, s.width as f64, s.height as f64);
        if label == "main" {
            g.visible = visible;
            g.x = x;
            g.y = y;
            g.w = w;
            g.h = h;
        } else if visible {
            aux.push((label, x, y, w, h));
        }
    }
    g.aux = aux;
    if let Ok(mut slot) = app.state::<SharedMainGeom>().0.lock() {
        *slot = g;
    }
}

/// 主线程：按"指针是否在桌宠区域"切换 main 窗口的鼠标穿透。
///
/// 无「输入监控」权限时 Default tap 建不出来（无法吞事件），
/// 此时用"悬停即接收"兜底：指针进桌宠区域 → 窗口接收鼠标（点击不落到桌面），
/// 指针离开 → 恢复穿透（不挡桌面）。
pub fn apply_pointer_hover<R: tauri::Runtime>(app: &AppHandle<R>, in_pet: bool) {
    let geom_state = app.state::<SharedMainGeom>();
    let actual_ignore = {
        let Ok(mut g) = geom_state.0.lock() else {
            return;
        };
        g.pointer_in_pet = in_pet;
        // 指针在桌宠上时永不穿透；不在时才尊重前端的 force_ignore 期望
        g.front_force_ignore && !in_pet
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_ignore_cursor_events(actual_ignore);
    }
}

/// 桌宠命中测试（纯内存计算，不碰窗口 API，可在 tap 线程安全调用）。
///
/// 判定链与官方 Windows per-region hit test 等价：
/// 1. main 可见且点在 main 的全局逻辑矩形内；
/// 2. 点不在本进程其他可见窗口内（那些点击属于辅助窗口自己）；
/// 3. 转为窗口内 CSS 逻辑坐标后命中 input_regions 或 visual_regions。
pub fn hit_test(app: &AppHandle, gx: f64, gy: f64) -> bool {
    let geom_state = app.state::<SharedMainGeom>();
    let Ok(g) = geom_state.0.lock() else {
        return false;
    };
    if !g.visible {
        return false;
    }
    if gx < g.x || gx > g.x + g.w || gy < g.y || gy > g.y + g.h {
        return false;
    }
    for (_label, x, y, w, h) in &g.aux {
        if gx >= *x && gx <= *x + *w && gy >= *y && gy <= *y + *h {
            return false;
        }
    }
    let (mx, my) = (g.x, g.y);
    drop(g);
    let lx = gx - mx;
    let ly = gy - my;
    let hit_state = app.state::<SharedMainHitState>();
    let Ok(st) = hit_state.0.lock() else {
        return false;
    };
    st.input_regions.iter().any(|r| r.contains(lx, ly))
        || st.visual_regions.iter().any(|r| r.contains(lx, ly))
}

/// 鼠标按下/释放 → `mouse-button`（前端据此合成点击/拖拽/右键菜单）。
/// 纯 emit，不查窗口几何，可从 tap 线程直接调用。
/// 注意：tap 回调里严禁同步写盘（会触发 tap 超时被系统禁用）。
pub fn handle_mouse_button(app: &AppHandle, _gx: f64, _gy: f64, right: bool, press: bool) {
    let _ = app.emit_to(
        "main",
        "mouse-button",
        serde_json::json!({
            "button": if right { "right" } else { "left" },
            "action": if press { "press" } else { "release" },
        }),
    );
}

// ---------------------------------------------------------------------------
// CGDisplay：全局逻辑点 → 物理像素（找鼠标所在显示器的 scale）
// ---------------------------------------------------------------------------

#[repr(C)]
struct CGPointRepr {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGRectRepr {
    origin: CGPointRepr,
    size: CGPointRepr,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetDisplaysWithPoint(
        point: CGPointRepr,
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRectRepr;
    fn CGDisplayPixelsWide(display: u32) -> usize;
}

/// 鼠标所在显示器的 backing scale（物理像素 / 逻辑点）。
/// 找不到显示器时退回 1.0。
fn display_scale_at(x: f64, y: f64) -> f64 {
    // SAFETY: 纯查询的系统调用，输出写入了调用方栈缓冲。
    unsafe {
        let mut displays: [u32; 8] = [0; 8];
        let mut count: u32 = 0;
        if CGGetDisplaysWithPoint(
            CGPointRepr { x, y },
            8,
            displays.as_mut_ptr(),
            &mut count,
        ) == 0 && count > 0
        {
            let bounds = CGDisplayBounds(displays[0]);
            let px_wide = CGDisplayPixelsWide(displays[0]);
            if bounds.size.x > 0.0 && px_wide > 0 {
                return px_wide as f64 / bounds.size.x;
            }
        }
        1.0
    }
}

/// CG 全局逻辑点 → 全局物理像素（前端 Ht() 期望的单位）
fn to_physical(x: f64, y: f64) -> (f64, f64) {
    let s = display_scale_at(x, y);
    (x * s, y * s)
}

// ---------------------------------------------------------------------------
// CGEventTap
// ---------------------------------------------------------------------------

/// mouse-move 最小发送间隔（约 60fps，合并 125Hz 鼠标的中间点）
const MOVE_THROTTLE_MS: i64 = 16;
/// 辅助功能权限轮询间隔
const PERMISSION_POLL: Duration = Duration::from_secs(3);
/// runloop 分片时长：每片检查一次重启标志
const RUN_LOOP_SLICE: f64 = 0.05;

// kCGEventFlagMask*（HIToolbox NX_* 设备无关位）
const MASK_SHIFT: u64 = 0x0002_0000;
const MASK_CONTROL: u64 = 0x0004_0000;
const MASK_ALTERNATE: u64 = 0x0008_0000;
const MASK_COMMAND: u64 = 0x0010_0000;

// 辅助功能权限（未授权时 CGEventTapCreate 返回 null）
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

// 输入监控权限（kTCCServiceListenEvent）。
// 区别（macOS 10.15+）：
// - ListenOnly tap（只观测）→ 只需「辅助功能 / Accessibility」
// - Default tap（可吞事件，防止点击穿透）→ 还需要「输入监控 / Input Monitoring」
// 缺输入监控时 CGEventTapCreate(Default) 直接返回 null，只能退回 ListenOnly，
// 桌宠区域的点击就会穿透到桌面。
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

fn listen_event_granted() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// 回调与 worker 共享的上下文。回调在其 runloop 线程上串行执行，
/// 仅原子计数需要跨线程。
struct TapContext {
    app: AppHandle,
    last_move_ms: AtomicI64,
    /// 本次按下是否命中了桌宠区域（命中则吞掉 press/drag/up，阻止事件穿透到桌面）
    swallowing: std::sync::atomic::AtomicBool,
    /// 指针当前是否悬停在桌宠区域（变化时切换窗口穿透状态）
    pointer_in_pet: std::sync::atomic::AtomicBool,
    /// tap 因超时被系统禁用时置位，由 worker 循环重新启用
    needs_tap_restart: Arc<std::sync::atomic::AtomicBool>,
}

/// 前端就绪握手（lib.rs 的 notify_input_bridge_ready 命令调用）。
/// 历史教训：曾按"emit 等握手"做门控，但前端初始化链中的 await 可能
/// 依赖首批输入事件（mouse-move 布局等），门控与链形成循环等待 →
/// 初始化链永久中断（hit region/监听注册全部不执行）。
/// 现在握手只做日志观测，事件始终直转，不做任何门控。
pub fn mark_input_bridge_ready<R: tauri::Runtime>(app: &AppHandle<R>) {
    use tauri::Manager;
    if let Some(st) = app.try_state::<crate::InputBridgeReady>() {
        let first = !st.0.swap(true, Ordering::AcqRel);
        if first {
            println!("[input] bridge handshake received (observer only, no gating)");
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 指针位置变化时切换窗口穿透状态（仅在跨越桌宠边界时 dispatch 到主线程）。
fn update_pointer_mode(ctx: &TapContext, gx: f64, gy: f64) {
    let in_pet = hit_test(&ctx.app, gx, gy);
    if ctx.pointer_in_pet.swap(in_pet, Ordering::AcqRel) != in_pet {
        let app = ctx.app.clone();
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || apply_pointer_hover(&app2, in_pet));
    }
}

fn emit_key(ctx: &TapContext, key: &str, pressed: bool) {
    let name = if pressed { "key-press" } else { "key-release" };
    let _ = ctx.app.emit_to("main", name, key);
}

fn key_code(event: &CGEvent) -> u16 {
    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode)
        .clamp(0, i64::from(u16::MAX)) as u16
}

/// macOS virtual keycode（HIToolbox `kVK_*`）→ `KeyboardEvent.key` 风格字符串。
/// 小键盘区经 BongoCat 的 keycode→HID usage 表校准。
fn map_key_code(code: u16) -> Option<&'static str> {
    Some(match code {
        0x00 => "A", 0x01 => "S", 0x02 => "D", 0x03 => "F",
        0x04 => "H", 0x05 => "G", 0x06 => "Z", 0x07 => "X",
        0x08 => "C", 0x09 => "V", 0x0B => "B", 0x0C => "Q",
        0x0D => "W", 0x0E => "E", 0x0F => "R", 0x10 => "Y",
        0x11 => "T",
        0x12 => "1", 0x13 => "2", 0x14 => "3", 0x15 => "4",
        0x16 => "6", 0x17 => "5", 0x18 => "=", 0x19 => "9",
        0x1A => "7", 0x1B => "-", 0x1C => "8", 0x1D => "0",
        0x1E => "]", 0x1F => "O", 0x20 => "U", 0x21 => "[",
        0x22 => "I", 0x23 => "P",
        0x24 => "Enter", 0x25 => "L", 0x26 => "J", 0x27 => "'",
        0x28 => "K", 0x29 => ";", 0x2A => "\\", 0x2B => ",",
        0x2C => "/", 0x2D => "N", 0x2E => "M", 0x2F => ".",
        0x30 => "Tab", 0x31 => " ", 0x32 => "`", 0x33 => "Backspace",
        0x35 => "Escape",
        // 修饰键正常路径走 FlagsChanged；此表兜底
        0x36 => "Meta", 0x37 => "Meta", 0x38 => "Shift",
        0x39 => "CapsLock", 0x3A => "Alt", 0x3B => "Control",
        0x3C => "Shift", 0x3D => "Alt", 0x3E => "Control", 0x3F => "Fn",
        // 小键盘
        0x41 => ".", 0x43 => "*", 0x45 => "+", 0x47 => "Clear",
        0x4B => "/", 0x4C => "Enter", 0x4E => "-",
        0x51 => "=", 0x52 => "0", 0x53 => "1", 0x54 => "2",
        0x55 => "3", 0x56 => "4", 0x57 => "5", 0x58 => "6",
        0x59 => "7", 0x5B => "8", 0x5C => "9",
        0x5F => ",", // JIS 小键盘逗号
        // 功能键（0x4F/0x50 为 F18/F19，勿与小键盘混淆）
        0x40 => "F17", 0x4F => "F18", 0x50 => "F19", 0x5A => "F20",
        0x60 => "F5", 0x61 => "F6", 0x62 => "F7", 0x63 => "F3",
        0x64 => "F8", 0x65 => "F9", 0x67 => "F11", 0x69 => "F13",
        0x6A => "F16", 0x6B => "F14", 0x6D => "F10", 0x6E => "ContextMenu",
        0x6F => "F12", 0x71 => "F15",
        // JIS 键盘
        0x66 => "Eisu", 0x68 => "Kana",
        // 导航
        0x72 => "Help", 0x73 => "Home", 0x74 => "PageUp", 0x75 => "Delete",
        0x76 => "F4", 0x77 => "End", 0x78 => "F2", 0x79 => "PageDown",
        0x7A => "F1",
        0x7B => "ArrowLeft", 0x7C => "ArrowRight",
        0x7D => "ArrowDown", 0x7E => "ArrowUp",
        _ => return None,
    })
}

/// FlagsChanged：按 keycode 定位修饰键、按 family flag 判断按下/释放。
/// 左右修饰键同键名，交替按住时 family 位保持置位导致的重复 press
/// 由前端 `Pr` Set 去重，无需设备位分析。
fn handle_flags_changed(ctx: &TapContext, code: u16, flags: u64) {
    let (key, mask) = match code {
        0x38 | 0x3C => ("Shift", MASK_SHIFT),
        0x3B | 0x3E => ("Control", MASK_CONTROL),
        0x3A | 0x3D => ("Alt", MASK_ALTERNATE),
        0x36 | 0x37 => ("Meta", MASK_COMMAND),
        0x39 => {
            // CapsLock 为开关型：发一对 press+release，避免前端按住集合残留
            emit_key(ctx, "CapsLock", true);
            emit_key(ctx, "CapsLock", false);
            return;
        }
        _ => return,
    };
    emit_key(ctx, key, flags & mask != 0);
}

/// 鼠标按下/释放 → `mouse-button`（前端据此合成点击/拖拽/右键菜单）
fn emit_mouse_button(ctx: &TapContext, gx: f64, gy: f64, right: bool, press: bool) {
    handle_mouse_button(&ctx.app, gx, gy, right, press);
}

unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: worker 在进程生命周期内持有 Box<TapContext>（循环不退出），
    // 回调不会观察到悬垂指针。
    let ctx = unsafe { &*user_info.cast::<TapContext>() };
    // SAFETY: CoreGraphics 保证回调期内 event 有效。
    let event_ref = unsafe { event.as_ref() };

    // ===== DEBUG: 每 100 个事件打印一次类型（用原子计数节流）=====
    static EVENT_COUNT: AtomicI64 = AtomicI64::new(0);
    let n = EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    if n % 500 == 0 {
        println!("[input] TAP CALLBACK fired #{n}, type={event_type:?}");
    }

    match event_type {
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let code = key_code(event_ref);
            match map_key_code(code) {
                Some(key) => {
                    emit_key(ctx, key, event_type == CGEventType::KeyDown);
                }
                None => {
                    // 未映射键低频记录（便于补齐映射表；写盘节流避免 tap 超时）
                    static UNMAPPED: AtomicI64 = AtomicI64::new(0);
                    let n = UNMAPPED.fetch_add(1, Ordering::Relaxed);
                    if n < 20 {
                        crate::dlog(&format!("[key] UNMAPPED code=0x{code:02X}"));
                    }
                }
            }
        }
        CGEventType::FlagsChanged => {
            let flags = CGEvent::flags(Some(event_ref)).bits();
            handle_flags_changed(ctx, key_code(event_ref), flags);
        }
        CGEventType::MouseMoved => {
            // 普通移动：先按位置切换穿透状态（跨越桌宠边界时才 dispatch），
            // 再按节流上报坐标给前端（前端据此更新 hover / 拖拽跟随）
            let loc = CGEvent::location(Some(event_ref));
            update_pointer_mode(ctx, loc.x, loc.y);
            let now = now_ms();
            if now - ctx.last_move_ms.load(Ordering::Relaxed) >= MOVE_THROTTLE_MS {
                ctx.last_move_ms.store(now, Ordering::Relaxed);
                let (x, y) = to_physical(loc.x, loc.y);
                let _ = ctx
                    .app
                    .emit_to("main", "mouse-move", serde_json::json!({ "x": x, "y": y }));
            }
        }
        CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            let loc = CGEvent::location(Some(event_ref));
            update_pointer_mode(ctx, loc.x, loc.y);
            let now = now_ms();
            if now - ctx.last_move_ms.load(Ordering::Relaxed) >= MOVE_THROTTLE_MS {
                ctx.last_move_ms.store(now, Ordering::Relaxed);
                let (x, y) = to_physical(loc.x, loc.y);
                let _ = ctx
                    .app
                    .emit_to("main", "mouse-move", serde_json::json!({ "x": x, "y": y }));
            }
            // 拖拽中：若本次按下命中了桌宠，吞掉 drag 事件，
            // 否则 macOS 会拿它去拖选桌面图标/窗口
            if ctx.swallowing.load(Ordering::Acquire) {
                return std::ptr::null_mut();
            }
        }
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown => {
            let loc = CGEvent::location(Some(event_ref));
            let right = event_type == CGEventType::RightMouseDown;
            // 命中测试用缓存几何（tap 线程禁止调窗口 API 与写盘）
            let hit = hit_test(&ctx.app, loc.x, loc.y);
            update_pointer_mode(ctx, loc.x, loc.y);
            // 无论是否命中都上报：前端内部用 elementFromPoint 再判定一次，
            // 未命中时它也会用 release 收尾（关闭 dock/菜单）
            emit_mouse_button(ctx, loc.x, loc.y, right, true);
            if hit {
                ctx.swallowing.store(true, Ordering::Release);
                // 吞掉：事件不再传给桌面，解决"点击桌宠穿透到桌面"
                return std::ptr::null_mut();
            }
        }
        CGEventType::LeftMouseUp | CGEventType::RightMouseUp => {
            let loc = CGEvent::location(Some(event_ref));
            let right = event_type == CGEventType::RightMouseUp;
            emit_mouse_button(ctx, loc.x, loc.y, right, false);
            if ctx.swallowing.swap(false, Ordering::AcqRel) {
                return std::ptr::null_mut();
            }
        }
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            println!("[input] event tap disabled ({event_type:?}), will restart");
            ctx.needs_tap_restart.store(true, Ordering::Release);
        }
        _ => {}
    }
    event.as_ptr()
}

fn event_mask() -> CGEventMask {
    [
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseUp,
    ]
    .into_iter()
    .fold(0, |mask, event_type| mask | (1_u64 << event_type.0))
}

/// 启动全局输入监听（非阻塞）。辅助功能未授权时每 3s 轮询并在日志提示，
/// 授权后自动建立 tap。worker 线程与进程同生命周期。
pub fn start(app: AppHandle) {
    thread::Builder::new()
        .name("global-input".into())
        .spawn(move || run_worker(app))
        .expect("spawn global-input thread");
}

fn run_worker(app: AppHandle) {
    // ===== 核心改动：不再先卡 AXIsProcessTrusted 检查！=====
    // AXIsProcessTrusted() 是**静默**查询，永远不会触发系统弹权限对话框。
    // macOS 只有在**真正调用 CGEventTapCreate** 时才会弹 "是否允许 WuwaTappo
    // 控制你的电脑" 的授权对话框。所以必须先尝试 CGEventTapCreate——
    // 即使第一次会返回 null（未授权），它也会触发弹窗。
    // 用户点允许后，tap 还在当前调用里不会自动成功（需要重新调用），
    // 所以这里用一个重试循环包着。

    #[allow(unused_assignments)]
    let mut tap_created_this_session = false;
    let max_attempts = 60; // 3 分钟，给足用户点允许的时间

    let needs_restart = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ctx = Box::new(TapContext {
        app,
        last_move_ms: AtomicI64::new(0),
        swallowing: std::sync::atomic::AtomicBool::new(false),
        pointer_in_pet: std::sync::atomic::AtomicBool::new(false),
        needs_tap_restart: Arc::clone(&needs_restart),
    });
    let ctx_ptr: *mut c_void = (&*ctx as *const TapContext).cast_mut().cast();

    // ===== 输入监控权限：Default tap（可吞事件）的前置条件 =====
    // 之前只查辅助功能，导致 Default tap 被系统拒绝、退回 ListenOnly，
    // 表现就是"点击桌宠穿透到桌面"。这里主动请求一次，让系统弹授权框。
    if !listen_event_granted() {
        println!(
            "[input] 输入监控(Input Monitoring)未授权 → 点击桌宠会穿透；正在请求系统授权"
        );
        let _ = std::fs::write(
            "/tmp/wuwatappo-input.log",
            "[input] Input Monitoring NOT granted\n\
             → 已请求系统授权；若未弹窗请手动：系统设置 → 隐私与安全性 → 输入监控 → 勾选 WuwaTappo\n\
             → 未授权时：点击/拖拽桌宠的事件会穿透到桌面\n",
        );
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            .spawn();
        // 弹系统授权对话框（每个 app 生命周期内仅首次有效）
        let _ = unsafe { CGRequestListenEventAccess() };
    }
    for attempt in 1..=max_attempts {
        // 有输入监控权限时才可能创建 Default（可吞事件）tap；
        // 否则先试 ListenOnly（只观测，点击会穿透），避免每轮空等。
        let can_swallow = listen_event_granted();
        let mut active_tap = can_swallow;
        let mut tap = if can_swallow {
            unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::HIDEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    event_mask(),
                    Some(event_tap_callback),
                    ctx_ptr,
                )
            }
        } else {
            None
        };
        if tap.is_none() {
            active_tap = false;
            tap = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::HIDEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    event_mask(),
                    Some(event_tap_callback),
                    ctx_ptr,
                )
            };
            if tap.is_some() && can_swallow {
                println!("[input] Default tap 被拒，退回 ListenOnly（点击会穿透）");
            }
        }
        if let Some(t) = tap {
            // ===== 成功了！=====
            tap_created_this_session = true;
            let tap = t;
            let _ = std::fs::write(
                "/tmp/wuwatappo-input.log",
                format!(
                    "[input] tap mode = {}\n",
                    if active_tap {
                        "default/swallow（输入监控已授权 → 桌宠区域点击不穿透）"
                    } else {
                        "listen-only（缺「输入监控」权限 → 点击会穿透到桌面）"
                    }
                ),
            );
            println!("[input] ✓ CGEventTap created on attempt #{attempt}");

            let source = match CFMachPort::new_run_loop_source(None, Some(&tap), 0) {
                Some(s) => s,
                None => {
                    println!("[input] create runloop source failed");
                    return;
                }
            };
            let run_loop = match CFRunLoop::current() {
                Some(rl) => rl,
                None => {
                    println!("[input] no current runloop");
                    return;
                }
            };
            let mode = match unsafe { kCFRunLoopDefaultMode } {
                Some(m) => m,
                None => {
                    println!("[input] kCFRunLoopDefaultMode unavailable");
                    return;
                }
            };
            run_loop.add_source(Some(&source), Some(mode));
            CGEvent::tap_enable(&tap, true);
            if !CGEvent::tap_is_enabled(&tap) {
                println!("[input] tap enable failed");
                return;
            }
            println!(
                "[input] CGEventTap running (HID head, {})",
                if active_tap { "default/swallow" } else { "listen-only" }
            );

            // ===== 分片运行：响应 tap 超时重启 + 输入监控授权后自动升级 =====
            let mut last_perm_check = now_ms();
            loop {
                CFRunLoop::run_in_mode(Some(mode), RUN_LOOP_SLICE, true);
                if needs_restart.swap(false, Ordering::AcqRel) {
                    println!("[input] re-enabling event tap after timeout");
                    CGEvent::tap_enable(&tap, true);
                }
                // ListenOnly 运行期间持续检查输入监控：一旦用户授权，
                // 立刻跳出重建为 Default tap（可吞事件），无需重启应用
                if !active_tap && now_ms() - last_perm_check >= 2000 {
                    last_perm_check = now_ms();
                    if listen_event_granted() {
                        println!("[input] 输入监控已授权 → 重建 Default tap（防穿透生效）");
                        let _ = std::fs::write(
                            "/tmp/wuwatappo-input.log",
                            "[input] Input Monitoring granted — rebuilding Default tap\n",
                        );
                        break;
                    }
                }
            }
            println!("[input] recreating event tap (upgrade to Default)");
            continue;
        } else {
            // ===== CGEventTapCreate 失败：要么权限未授予，要么 macOS 需要弹窗 =====
            if attempt == 1 {
                // 第一次失败：打开系统设置 + 写日志，让用户知道需要授权
                let _ = std::fs::write(
                    "/tmp/wuwatappo-input.log",
                    "[input] CGEventTapCreate failed (accessibility not granted?)\n\
                     → 系统应该弹出 '是否允许 WuwaTappo 控制你的电脑' 的对话框\n\
                     → 点'允许'，或在 系统设置 → 隐私与安全性 → 无障碍 中手动勾选\n",
                );
                let _ = std::process::Command::new("open")
                    .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                    .spawn();
                println!(
                    "[input] CGEventTapCreate attempt #1 failed → 已打开系统设置等待授权（最多 {} 次重试）",
                    max_attempts
                );
                let mut _unused = unsafe { AXIsProcessTrusted() }; // 保留符号链接
            }
            // 等 3 秒再试
            thread::sleep(std::time::Duration::from_secs(3));
            if attempt % 10 == 0 {
                println!("[input] still waiting for accessibility... ({attempt}/{max_attempts})");
            }
        }
    }

    // 3 分钟内都没成功
    if !tap_created_this_session {
        let _ = std::fs::write(
            "/tmp/wuwatappo-input.log",
            "[input] CGEventTapCreate failed after all retries\n\
             → 请手动检查：系统设置 → 隐私与安全性 → 无障碍 → 勾选 WuwaTappo\n\
             → 或检查终端有没有弹出授权对话框没点\n",
        );
        println!("[input] FATAL: CGEventTapCreate failed after {max_attempts} attempts. Check Accessibility permission.");
    }
}
