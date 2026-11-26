//! Hand a file path or URL to the system's default handler.

/// Open `target` with the OS default application or browser.
///
/// Returns the shell's failure reason instead of silently doing nothing; the
/// caller decides whether a failed open is worth surfacing to the user.
pub fn open(target: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let target_utf16: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: every string argument is NUL-terminated UTF-16 and outlives
        // the call; a null owner window is valid for a plain shell open.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target_utf16.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOW,
            )
        } as isize;
        // ShellExecuteW returns a value > 32 on success; <= 32 is an error.
        if result <= 32 {
            return Err(format!("ShellExecuteW failed with code {result}"));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
