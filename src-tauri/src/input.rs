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

/// 宽松解析前端 regions 数组：字段值可能是数字或字符串数字，含 href 等附加字段忽略
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
            if let (Some(x), Some(y), Some(w), Some(h)) =
                (f("x"), f("y"), f("width"), f("height"))
            {
                out.push(RegionRect { x, y, w, h });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// mouse-button 路由（官方 per-region 语义的 macOS 等价）
// ---------------------------------------------------------------------------

/// mouse-down 是否路由给 main 的原生输入桥。
/// 官方 Windows：Win32 per-region hit test，仅桌宠实体/交互区域的事件
/// 进入 native input POC。macOS 等价判断（主线程）：
/// 1. 点在 main 窗口内且 main 可见；
/// 2. 点不在本进程其他可见窗口（login/panel/about/toast 等）内——
///    否则点击属于那个窗口，不能让穿透层误响应；
/// 3. 点命中 input_regions 或 visual_regions。
/// mouse-up 不过滤：拖拽/手势收尾必须总能送达。
fn route_mouse_press(app: &AppHandle, gx: f64, gy: f64) -> bool {
    let Some(main) = app.get_webview_window("main") else {
        return false;
    };
    if !main.is_visible().unwrap_or(false) {
        return false;
    }
    let (Ok(pos), Ok(size)) = (main.outer_position(), main.outer_size()) else {
        return false;
    };
    let (mpx, mpy) = (pos.x as f64, pos.y as f64);
    let (mw, mh) = (size.width as f64, size.height as f64);
    if gx < mpx || gx > mpx + mw || gy < mpy || gy > mpy + mh {
        return false;
    }
    // 辅助窗口遮挡（它们不穿透，点击应归属它们自己）
    for (label, win) in app.webview_windows() {
        if label == "main" {
            continue;
        }
        let Ok(true) = win.is_visible() else { continue };
        let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size()) else { continue };
        let (x, y, w, h) = (p.x as f64, p.y as f64, s.width as f64, s.height as f64);
        if gx >= x && gx <= x + w && gy >= y && gy <= y + h {
            return false;
        }
    }
    // 全局物理坐标 → main 窗口内 CSS 逻辑坐标
    let scale = main.scale_factor().unwrap_or(1.0);
    if scale <= 0.0 {
        return false;
    }
    let lx = (gx - mpx) / scale;
    let ly = (gy - mpy) / scale;
    let st = app
        .state::<SharedMainHitState>()
        .0
        .lock()
        .map(|s| (s.input_regions.clone(), s.visual_regions.clone()))
        .unwrap_or_default();
    let (inputs, visuals) = st;
    inputs.iter().any(|r| r.contains(lx, ly)) || visuals.iter().any(|r| r.contains(lx, ly))
}

/// 鼠标按下/释放 → `mouse-button`（前端据此合成点击/拖拽/右键菜单）。
/// 必须主线程调用（窗口几何查询）。
pub fn handle_mouse_button(app: &AppHandle, gx: f64, gy: f64, right: bool, press: bool) {
    if press && !route_mouse_press(app, gx, gy) {
        return;
    }
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

/// 回调与 worker 共享的上下文。回调在其 runloop 线程上串行执行，
/// 仅原子计数需要跨线程。
struct TapContext {
    app: AppHandle,
    last_move_ms: AtomicI64,
    /// tap 因超时被系统禁用时置位，由 worker 循环重新启用
    needs_tap_restart: Arc<std::sync::atomic::AtomicBool>,
    /// 前端就绪握手（notify_input_bridge_ready 置位）。
    /// tap 若从进程启动即高频 emit，会在 WebView 完成初始化前轰炸 IPC，
    /// 与前端 onMounted 初始化链（监听注册）产生竞态，导致
    /// key-press/mouse-button 监听注册失败（计数与桌宠交互全部失效，
    /// 且现象随编译产物时序漂移、时好时坏）。故 emit 必须等握手。
    bridge_ready: Arc<std::sync::atomic::AtomicBool>,
    /// tap 建立时刻：握手丢失时的兜底（10s 后放行，防永久死锁）
    tap_started: std::time::Instant,
}

impl TapContext {
    /// 事件是否可以转发给前端：已握手，或超过兜底时限
    fn bridge_open(&self) -> bool {
        self.bridge_ready.load(Ordering::Acquire)
            || self.tap_started.elapsed() > Duration::from_secs(10)
    }
}

/// 前端就绪握手（lib.rs 的 notify_input_bridge_ready 命令调用）
pub fn mark_input_bridge_ready<R: tauri::Runtime>(app: &AppHandle<R>) {
    use tauri::Manager;
    if let Some(st) = app.try_state::<crate::InputBridgeReady>() {
        st.0.store(true, Ordering::Release);
        println!("[input] bridge handshake received, event forwarding enabled");
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn emit_key(ctx: &TapContext, key: &str, pressed: bool) {
    if !ctx.bridge_open() {
        return;
    }
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
    if !ctx.bridge_open() {
        return;
    }
    let app = ctx.app.clone();
    let closure_app = app.clone();
    // 命中路由需查窗口几何（主线程）；press 事件被过滤时不 emit
    let _ = app.run_on_main_thread(move || {
        handle_mouse_button(&closure_app, gx, gy, right, press);
    });
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

    match event_type {
        CGEventType::KeyDown | CGEventType::KeyUp => {
            if let Some(key) = map_key_code(key_code(event_ref)) {
                emit_key(ctx, key, event_type == CGEventType::KeyDown);
            }
        }
        CGEventType::FlagsChanged => {
            let flags = CGEvent::flags(Some(event_ref)).bits();
            handle_flags_changed(ctx, key_code(event_ref), flags);
        }
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            if !ctx.bridge_open() {
                return event.as_ptr();
            }
            let now = now_ms();
            if now - ctx.last_move_ms.load(Ordering::Relaxed) >= MOVE_THROTTLE_MS {
                ctx.last_move_ms.store(now, Ordering::Relaxed);
                let loc = CGEvent::location(Some(event_ref));
                let (x, y) = to_physical(loc.x, loc.y);
                let _ = ctx.app.emit_to("main", "mouse-move", serde_json::json!({ "x": x, "y": y }));
            }
        }
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown => {
            let loc = CGEvent::location(Some(event_ref));
            let (x, y) = to_physical(loc.x, loc.y);
            emit_mouse_button(ctx, x, y, event_type == CGEventType::RightMouseDown, true);
        }
        CGEventType::LeftMouseUp | CGEventType::RightMouseUp => {
            let loc = CGEvent::location(Some(event_ref));
            let (x, y) = to_physical(loc.x, loc.y);
            emit_mouse_button(ctx, x, y, event_type == CGEventType::RightMouseUp, false);
        }
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            println!("[input] event tap disabled ({event_type:?}), will restart");
            ctx.needs_tap_restart.store(true, Ordering::Release);
        }
        _ => {}
    }
    // ListenOnly tap：事件原样透传
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
    // 1) 等待辅助功能权限。
    // adhoc 签名（signingIdentity "-"）的 app 每次重新编译签名都会变化，
    // macOS 会视为新应用导致授权失效——此处必须给用户可见提示：
    // 打开系统设置的辅助功能面板，并在日志/stdout 双通道记录。
    let mut prompted = false;
    loop {
        // SAFETY: 纯查询的系统调用
        if unsafe { AXIsProcessTrusted() } {
            break;
        }
        if !prompted {
            prompted = true;
            println!(
                "[input] 辅助功能未授权：已打开系统设置，请在 隐私与安全性 → 辅助功能 中勾选 WuwaTappo（授权后自动生效）"
            );
            let _ = std::fs::write(
                "/tmp/wuwatappo-input.log",
                "[input] accessibility NOT granted; waiting for user approval\n",
            );
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .spawn();
        }
        thread::sleep(PERMISSION_POLL);
    }
    println!("[input] accessibility granted, creating CGEventTap");

    // 2) 建立 tap；context 由 Box 固定地址，worker 循环不退出故不会被释放
    let needs_restart = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let bridge_ready = {
        use tauri::Manager;
        app.try_state::<crate::InputBridgeReady>()
            .map(|s| Arc::clone(&s.0))
            .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(false)))
    };
    let ctx = Box::new(TapContext {
        app,
        last_move_ms: AtomicI64::new(0),
        needs_tap_restart: Arc::clone(&needs_restart),
        bridge_ready,
        tap_started: std::time::Instant::now(),
    });
    let ctx_ptr: *mut c_void = (&*ctx as *const TapContext).cast_mut().cast();

    // SAFETY: ctx 指向本线程持有的稳定 Box 分配；tap 为 ListenOnly。
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            event_mask(),
            Some(event_tap_callback),
            ctx_ptr,
        )
    };
    let tap = match tap {
        Some(t) => t,
        None => {
            println!("[input] CGEventTapCreate failed（权限可能被撤销），全局键鼠监听停用");
            return;
        }
    };
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
    // SAFETY: 进程级不可变常量
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
    println!("[input] CGEventTap running (HID head, listen-only)");

    // 3) 分片运行：响应 tap 超时重启
    loop {
        CFRunLoop::run_in_mode(Some(mode), RUN_LOOP_SLICE, true);
        if needs_restart.swap(false, Ordering::AcqRel) {
            println!("[input] re-enabling event tap after timeout");
            CGEvent::tap_enable(&tap, true);
        }
    }
}
