//! Mouse control — native Win32 / enigo on macOS / PlatformNotSupported on Linux

use crate::core::*;

/// Create a shared enigo instance (macOS / Linux)
#[cfg(not(windows))]
fn enigo() -> std::sync::Mutex<enigo::Enigo> {
    static INST: std::sync::OnceLock<std::sync::Mutex<enigo::Enigo>> = std::sync::OnceLock::new();
    INST.get_or_init(|| std::sync::Mutex::new(enigo::Enigo::new(&enigo::Settings::default()))).clone()
}

/// Move the mouse to the given coordinates
pub async fn move_to(x: i32, y: i32) -> Result<()> {
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
        unsafe { let _ = SetCursorPos(x, y); }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?
            .move_mouse(x as i32, y as i32)
            .map_err(|e| DesktopError::InputFailed(e.to_string()))
    }
    #[cfg(all(not(windows), not(any(target_os = "macos", target_os = "linux"))))]
    {
        Err(DesktopError::PlatformNotSupported)
    }
}

/// Get the current mouse position
pub async fn position() -> Result<Point> {
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        use ::windows::Win32::Foundation::POINT;
        let mut pt = POINT::default();
        unsafe { let _ = GetCursorPos(&mut pt); }
        Ok(Point { x: pt.x, y: pt.y })
    }
    #[cfg(not(windows))]
    {
        let pos = enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?
            .location()
            .map_err(|e| DesktopError::InputFailed(e.to_string()))?;
        Ok(Point { x: pos.0 as i32, y: pos.1 as i32 })
    }
    #[cfg(all(not(windows), not(any(target_os = "macos", target_os = "linux"))))]
    {
        Err(DesktopError::PlatformNotSupported)
    }
}

/// Click the mouse
pub async fn click(x: i32, y: i32) -> Result<()> {
    move_to(x, y).await?;
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        };
        unsafe { mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); std::thread::sleep(std::time::Duration::from_millis(50)); mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0); }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?
            .button_click(enigo::MouseButton::Left)
            .map_err(|e| DesktopError::InputFailed(e.to_string()))
    }
    #[cfg(all(not(windows), not(any(target_os = "macos", target_os = "linux"))))]
    {
        Err(DesktopError::PlatformNotSupported)
    }
}

/// Right-click the mouse
pub async fn right_click(x: i32, y: i32) -> Result<()> {
    move_to(x, y).await?;
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            mouse_event, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        };
        unsafe { mouse_event(MOUSEEVENTF_RIGHTDOWN, 0, 0, 0, 0); std::thread::sleep(std::time::Duration::from_millis(50)); mouse_event(MOUSEEVENTF_RIGHTUP, 0, 0, 0, 0); }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?
            .button_click(enigo::MouseButton::Right)
            .map_err(|e| DesktopError::InputFailed(e.to_string()))
    }
    #[cfg(all(not(windows), not(any(target_os = "macos", target_os = "linux"))))]
    {
        Err(DesktopError::PlatformNotSupported)
    }
}

/// Scroll the mouse wheel
///
/// `direction`: "up" / "down"; `amount`: number of wheel notches (each notch = 120 delta).
pub async fn scroll(direction: &str, amount: i32) -> Result<()> {
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_WHEEL};
        let delta: i32 = if direction == "up" { 120 } else { -120 };
        for _ in 0..amount.max(0) {
            unsafe { mouse_event(MOUSEEVENTF_WHEEL, 0, 0, delta, 0); }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (direction, amount);
        Err(DesktopError::PlatformNotSupported)
    }
}

/// Drag the mouse
pub async fn drag(start: Point, end: Point) -> Result<()> {
    move_to(start.x, start.y).await?;
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        };
        unsafe { mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); }
        let steps = 20;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = (start.x as f32 + (end.x as f32 - start.x as f32) * t) as i32;
            let y = (start.y as f32 + (end.y as f32 - start.y as f32) * t) as i32;
            let _ = move_to(x, y).await;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        unsafe { mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0); }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut e = enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?;
        e.button_down(enigo::MouseButton::Left).map_err(|e| DesktopError::InputFailed(e.to_string()))?;
        drop(e);
        for i in 1..=20 {
            let t = i as f32 / 20.0;
            let x = (start.x as f32 + (end.x as f32 - start.x as f32) * t) as i32;
            let y = (start.y as f32 + (end.y as f32 - start.y as f32) * t) as i32;
            let _ = move_to(x, y).await;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut e = enigo().lock().map_err(|e| DesktopError::InputFailed(e.to_string()))?;
        e.button_up(enigo::MouseButton::Left).map_err(|e| DesktopError::InputFailed(e.to_string()))
    }
    #[cfg(all(not(windows), not(any(target_os = "macos", target_os = "linux"))))]
    {
        Err(DesktopError::PlatformNotSupported)
    }
}