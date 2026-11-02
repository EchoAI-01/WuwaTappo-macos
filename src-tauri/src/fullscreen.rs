//! 前台应用全屏检测（1.0.3 游戏模式数据源）。
//!
//! 官方 Windows 端在 `get_main_desktop_safe_area` 返回
//! `foregroundApplicationFullscreen`，前端 `autoGameModeOnFullscreen` 据此
//! 自动进入/退出游戏模式（禁用桌宠交互）。macOS 无 Win32 前台窗口 API，
//! 用 CGWindowList 等价实现：遍历在屏窗口，存在 layer==0（普通应用层）、
//! 非本进程、bounds 完整覆盖主显示器的窗口即判定"有应用处于全屏"。
//!
//! 说明：
//! - 窗口列表（layer/bounds/pid）不需要屏幕录制权限（仅窗口名需要）；
//! - 普通"最大化"窗口不覆盖菜单栏（y>0），不会误判；全屏 Space 窗口
//!   bounds == 整屏（含菜单栏区域）；
//! - 桌宠自身 main=floating(3)/aux=4 层，天然被 layer==0 过滤排除。

use std::ffi::c_void;

type CFType = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    w: f64,
    h: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    o: CGPoint,
    s: CGSize,
}

extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFType;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFType, rect: *mut CGRect) -> bool;
    fn CFArrayGetCount(array: CFType) -> isize;
    fn CFArrayGetValueAtIndex(array: CFType, idx: isize) -> CFType;
    fn CFDictionaryGetValue(dict: CFType, key: CFType) -> CFType;
    fn CFNumberGetValue(number: CFType, the_type: i32, value_ptr: *mut c_void) -> bool;
    fn CFRelease(cf: CFType);
    static kCGWindowLayer: CFType;
    static kCGWindowBounds: CFType;
    static kCGWindowOwnerPID: CFType;
}

const K_CF_NUMBER_S_INT32: i32 = 3;
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_NULL_WINDOW_ID: u32 = 0;

fn cf_num(v: CFType) -> Option<i64> {
    if v.is_null() {
        return None;
    }
    let mut out: i32 = 0;
    unsafe {
        if CFNumberGetValue(v, K_CF_NUMBER_S_INT32, &mut out as *mut i32 as *mut c_void) {
            Some(out as i64)
        } else {
            None
        }
    }
}

/// 当前屏幕上是否存在其他应用的全屏窗口。
pub fn foreground_application_fullscreen() -> bool {
    unsafe {
        let my_pid = std::process::id() as i64;
        let screen = CGDisplayBounds(CGMainDisplayID());
        let list = CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY,
            K_CG_NULL_WINDOW_ID,
        );
        if list.is_null() {
            return false;
        }
        let mut result = false;
        let count = CFArrayGetCount(list);
        for i in 0..count {
            let w = CFArrayGetValueAtIndex(list, i);
            if w.is_null() {
                continue;
            }
            // 仅普通应用窗口层（排除桌面、菜单栏、浮动层、我们自己的窗口）
            if cf_num(CFDictionaryGetValue(w, kCGWindowLayer)) != Some(0) {
                continue;
            }
            if cf_num(CFDictionaryGetValue(w, kCGWindowOwnerPID)) == Some(my_pid) {
                continue;
            }
            let bounds_dict = CFDictionaryGetValue(w, kCGWindowBounds);
            if bounds_dict.is_null() {
                continue;
            }
            let mut r = CGRect {
                o: CGPoint { x: 0.0, y: 0.0 },
                s: CGSize { w: 0.0, h: 0.0 },
            };
            if CGRectMakeWithDictionaryRepresentation(bounds_dict, &mut r)
                && r.o.x <= screen.o.x
                && r.o.y <= screen.o.y
                && r.o.x + r.s.w >= screen.o.x + screen.s.w - 0.5
                && r.o.y + r.s.h >= screen.o.y + screen.s.h - 0.5
            {
                result = true;
                break;
            }
        }
        CFRelease(list);
        result
    }
}
