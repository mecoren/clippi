//! The system's animation-effects preference.
//!
//! Windows exposes it as `SPI_GETCLIENTAREAANIMATION` (the "Animation effects"
//! switch under Accessibility → Visual effects), macOS as the "Reduce motion"
//! switch. The settings page can override it per animation channel; when a
//! channel follows the system, [`crate::ui::motion_pref`] reads it here.
//!
//! The reading is done here rather than through `gpui_base`'s
//! `apply_system_reduce_motion` because that helper stops applying platform
//! values once the application has called `App::set_reduce_motion` — and this
//! app does exactly that whenever a preference is loaded or changed.

#[cfg(target_os = "windows")]
#[path = "motion_windows.rs"]
mod implementation;
#[cfg(target_os = "macos")]
#[path = "motion_macos.rs"]
mod implementation;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod implementation {
    /// Animations are assumed on where the platform cannot be asked.
    pub fn system_animations_enabled() -> bool {
        true
    }
}

pub use implementation::system_animations_enabled;
