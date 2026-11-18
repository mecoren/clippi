//! Windows reading of the "Animation effects" preference.

use std::ffi::c_void;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
};

/// Whether the system's animation effects are on. An unreadable value counts
/// as on: animation is the documented default, and a silent failure must not
/// switch the whole interface's motion off.
pub fn system_animations_enabled() -> bool {
    let mut animations_enabled: i32 = 0;
    // SAFETY: `SystemParametersInfoW` with `SPI_GETCLIENTAREAANIMATION` writes
    // one BOOL through `pvParam` and ignores `uiParam`; the BOOL outlives the
    // call and the flags ask for no update.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut animations_enabled as *mut i32 as *mut c_void,
            0,
        )
    };
    if ok == 0 {
        return true;
    }
    animations_enabled != 0
}
