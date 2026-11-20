//! Native window policy for the card → flyout connector overlay.
//!
//! The connector needs a surface spanning the hovered card (inside the main
//! window) and the preview flyout (outside it), so neither of those windows
//! can paint it. This module owns a dedicated transparent overlay window:
//!
//! * it is owned by the flyout, so it always floats above both the flyout
//!   and the main window;
//! * it is fully click-through — every mouse message falls through to the
//!   windows underneath, keeping the list's hover machinery intact;
//! * it never activates and never takes focus.
//!
//! All coordinates handed to [`Overlay::place`] are logical screen pixels
//! with a top-left origin.

#[cfg(target_os = "windows")]
#[path = "preview_overlay_windows.rs"]
mod implementation;
#[cfg(target_os = "macos")]
#[path = "preview_overlay_macos.rs"]
mod implementation;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use implementation::Overlay;

/// Non-Windows/macOS builds have no connector; the caller skips the line.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct Overlay;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Overlay {
    pub fn install(_owner: isize, _overlay: isize) -> Result<std::rc::Rc<Self>, &'static str> {
        Err("native connector overlay is unavailable on this platform")
    }

    pub fn place(&self, _x: f32, _y: f32, _width: f32, _height: f32) -> bool {
        false
    }

    pub fn hide(&self) {}
}
