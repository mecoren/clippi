//! Fatal startup dialog.
//!
//! Used before any GPUI window exists (the database failed to open), so it has
//! to rely on the native toolkit rather than the app's own components.

/// Show a blocking error message. Returns after the user dismisses it.
#[cfg(target_os = "windows")]
pub fn show_fatal(title: &str, message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };

    let wide = |text: &str| -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let title = wide(title);
    let message = wide(message);
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the
    // call; a null owner window is valid for a task-modal message box.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(target_os = "macos")]
pub fn show_fatal(title: &str, message: &str) {
    // `osascript` ships with every macOS install; a native NSAlert would need
    // an initialized NSApplication, which does not exist yet at this point.
    let script = format!(
        "display alert {} message {} as critical buttons {{\"OK\"}} default button \"OK\"",
        apple_script_string(title),
        apple_script_string(message)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

#[cfg(target_os = "macos")]
fn apple_script_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn show_fatal(title: &str, message: &str) {
    log::error!("{title}: {message}");
}
