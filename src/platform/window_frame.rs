//! Native window frame policy for the main window.
//!
//! DWM owns the frame: it draws the rounded corners, the 1px window stroke and
//! the drop shadow, so the panel fills the client area as a square and gets
//! clipped by the system frame. The client still covers the whole window
//! rectangle, which keeps the painted content flush with the window edges while
//! `WS_THICKFRAME` keeps the native resize border alive.

#[cfg(target_os = "windows")]
#[path = "window_frame_windows.rs"]
mod implementation;

#[cfg(target_os = "windows")]
pub use implementation::install;

/// Apply the main window's DWM frame policy to another top-level window.
///
/// Windows: rounded corners, 1 px stroke, shadow and `WS_THICKFRAME` resize
/// edges. Other platforms keep their native decoration (no-op here).
pub fn install_for(window: &gpui::Window) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                install(win32.hwnd.get());
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
    }
}
