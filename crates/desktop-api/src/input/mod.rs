//! Input control module - SendInput Unicode + mouse + keyboard

use crate::core::*;

#[cfg(windows)]
pub mod sendinput;
pub mod mouse;
pub mod keyboard;

#[cfg(windows)]
pub use sendinput::*;
pub use mouse::*;
pub use keyboard::*;

/// Input engine
pub struct InputEngine;

impl Default for InputEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputEngine {
    pub fn new() -> Self {
        Self
    }

    /// Send text - automatically picks the optimal strategy
    pub async fn send_text(&self, text: &str, target: &mut Target) -> Result<()> {
        // Ensure the target is active
        self.ensure_active(target).await?;

        match target {
            #[cfg(windows)]
            Target::Window { .. } | Target::Tui { .. } => {
                // Windows: SendInput Unicode
                sendinput::send_unicode_text(text)?;
            }
            #[cfg(not(windows))]
            Target::Tui { .. } => {
                // Non-Windows: no TUI text-input path yet (Tui is effectively a
                // Windows concept); keep the match exhaustive.
            }
            Target::Browser { .. } => {
                // Browser: Playwright input
                // TODO: call the browser module
            }
        }

        Ok(())
    }

    /// Click - auto-activate + click
    pub async fn click(&self, target: &mut Target, point: Point) -> Result<()> {
        self.ensure_active(target).await?;
        mouse::click(point.x, point.y).await
    }

    /// Activate the target window to the foreground (idempotent: skipped if already verified).
    ///
    /// Standalone window-activation entrypoint (originally an internal ensure_active capability), for
    /// consumers that need "activate only, no operation" (e.g. nuphus-mcp's desktop_window_activate).
    pub async fn activate(&self, target: &mut Target) -> Result<()> {
        self.ensure_active(target).await
    }

    /// Drag
    pub async fn drag(&self, target: &mut Target, start: Point, end: Point) -> Result<()> {
        self.ensure_active(target).await?;
        mouse::drag(start, end).await
    }

    /// Key press
    pub async fn press(&self, target: &mut Target, key: &str) -> Result<()> {
        self.ensure_active(target).await?;
        keyboard::press(key).await
    }

    /// Key combination
    pub async fn hotkey(&self, target: &mut Target, keys: &[&str]) -> Result<()> {
        self.ensure_active(target).await?;
        keyboard::hotkey(keys).await
    }

    /// Ensure the target is active (built-in)
    async fn ensure_active(&self, target: &mut Target) -> Result<()> {
        if target.is_verified() {
            return Ok(());
        }

        #[cfg(windows)]
        {
            use ::windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
            use ::windows::Win32::Foundation::HWND;
            use std::time::Duration;
            use tokio::time::sleep;

            let hwnd = match target {
                Target::Window { hwnd, .. } => *hwnd,
                Target::Tui { hwnd, .. } => *hwnd,
                _ => return Ok(()),
            };

            let handle = HWND(hwnd);
            unsafe {
                let _ = SetForegroundWindow(handle);
            }
            sleep(Duration::from_millis(100)).await;

            // Verify whether it is in the foreground
            if !self.is_foreground(hwnd) {
                // Force foreground: AttachThreadInput
                self.force_foreground(hwnd).await?;
            }
        }

        target.verify();
        Ok(())
    }

    #[cfg(windows)]
    fn is_foreground(&self, hwnd: isize) -> bool {
        use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        unsafe {
            let fg = GetForegroundWindow();
            fg.0 as isize == hwnd
        }
    }

    #[cfg(windows)]
    async fn force_foreground(&self, hwnd: isize) -> Result<()> {
        use ::windows::Win32::UI::WindowsAndMessaging::{
            GetWindowThreadProcessId, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        use ::windows::Win32::System::Threading::{GetCurrentThreadId, AttachThreadInput};
        use ::windows::Win32::Foundation::HWND;
        use std::time::Duration;
        use tokio::time::sleep;

        let handle = HWND(hwnd);
        let target_tid = unsafe { GetWindowThreadProcessId(handle, None) };
        let current_tid = unsafe { GetCurrentThreadId() };

        unsafe {
            let _ = AttachThreadInput(current_tid, target_tid, true);
            let _ = ShowWindow(handle, SW_RESTORE);
            let _ = SetForegroundWindow(handle);
            let _ = AttachThreadInput(current_tid, target_tid, false);
        }

        sleep(Duration::from_millis(200)).await;
        Ok(())
    }
}