//! Whether the main window fades as it shows and hides, and how long it takes.
//!
//! The preference is resolved by [`crate::ui::motion_pref`] from the
//! `window_motion` setting (or the system's animation-effects switch when the
//! setting follows it) and stored here; the window code only reads it.
//!
//! The fade itself is drawn by the view layer (`ui::root`): the main window is
//! created with `WindowBackgroundAppearance::Transparent`, which gpui turns
//! into a DirectComposition window without a redirection surface
//! (`WS_EX_NOREDIRECTIONBITMAP`). Such a window cannot take `WS_EX_LAYERED`
//! constant alpha, so the panel's own opacity is animated instead. That also
//! keeps DWM's rounded corners and the system stroke untouched.
//!
//! Only the main window fades. The quick paste window and the tag sidebar stay
//! instantaneous: they answer a keystroke and a click, where a fade would read
//! as lag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 显示淡入时长（面板整体透明度 0 → 1）。
pub fn fade_in() -> Duration {
    Duration::from_millis(180)
}

/// 隐藏淡出时长（面板整体透明度 1 → 0，结束后才真正隐藏）。
pub fn fade_out() -> Duration {
    Duration::from_millis(140)
}

#[cfg(test)]
mod tests {
    use super::{fade_in, fade_out};

    #[test]
    fn both_fades_stay_short() {
        assert!(fade_in().as_millis() <= 220);
        assert!(fade_out().as_millis() <= 180);
        // 淡出不得长于淡入：关闭要跟手。
        assert!(fade_out() <= fade_in());
    }
}
