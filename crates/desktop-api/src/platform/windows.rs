//! Windows platform-specific implementation

use crate::core::*;
use crate::platform::WindowInfo;

/// Windows window enumeration callback
///
/// # Safety
/// - `hwnd` must be a valid window handle
/// - `lparam` must point to a valid `Vec<WindowInfo>`
/// - The caller must ensure the correctness of the Windows enumeration context
pub unsafe extern "system" fn enum_callback(hwnd: ::windows::Win32::Foundation::HWND, lparam: ::windows::Win32::Foundation::LPARAM) -> ::windows::Win32::Foundation::BOOL {
    use ::windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, IsWindowVisible, GetWindowRect, GetWindowThreadProcessId};
    use ::windows::Win32::Foundation::{BOOL, RECT};

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len == 0 {
        return BOOL(1);
    }

    let title = String::from_utf16_lossy(&buf[..len as usize]);
    let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);

    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);

    // Get the process ID
    let mut pid: u32 = 0;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));

    windows.push(WindowInfo {
        hwnd: hwnd.0,
        title,
        x: rect.left,
        y: rect.top,
        width: (rect.right - rect.left) as u32,
        height: (rect.bottom - rect.top) as u32,
        visible: true,
        process_id: pid,
    });

    BOOL(1)
}

/// Windows window search callback
///
/// # Safety
/// - `hwnd` must be a valid window handle
/// - `lparam` must point to a valid `SearchCtx`
/// - The caller must ensure the correctness of the Windows enumeration context
pub unsafe extern "system" fn search_callback(hwnd: ::windows::Win32::Foundation::HWND, lparam: ::windows::Win32::Foundation::LPARAM) -> ::windows::Win32::Foundation::BOOL {
    use ::windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, IsWindowVisible};
    use ::windows::Win32::Foundation::BOOL;

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len == 0 {
        return BOOL(1);
    }

    let title = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
    let ctx = &mut *(lparam.0 as *mut SearchCtx);

    if title.contains(&ctx.query) {
        ctx.result = Some(hwnd.0);
        return BOOL(0);
    }

    BOOL(1)
}

/// Detect the window graphics backend
pub fn detect_gfx_backend(hwnd: isize) -> GfxBackend {
    use ::windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    use ::windows::Win32::Foundation::HWND;

    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(HWND(hwnd), &mut buf) };
    if len == 0 {
        return GfxBackend::Unknown;
    }

    let class = String::from_utf16_lossy(&buf[..len as usize]);
    match class.as_str() {
        "Chrome_WidgetWin_1" | "Chrome_WidgetWin_2" | "CefBrowserWindow" => GfxBackend::DirectX,
        "Qt5QWindowIcon" | "Qt6QWindowIcon" => GfxBackend::OpenGl,
        "ConsoleWindowClass" | "#32770" => GfxBackend::Gdi,
        _ => GfxBackend::Gdi,
    }
}

/// Search context
pub struct SearchCtx {
    pub query: String,
    pub result: Option<isize>,
}