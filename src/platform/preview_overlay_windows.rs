//! Windows implementation of the connector overlay policy.
//!
//! Input fall-through uses `WM_NCHITTEST → HTTRANSPARENT`: the system then
//! re-dispatches the hit test to windows below in the same thread (the main
//! window), which is exactly the ownership group this overlay lives in. The
//! window is owned by the flyout (`GWLP_HWNDPARENT`) so it also stacks above
//! the flyout while the main window stays at the bottom of the group.

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Dwm::{
        DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    },
    UI::HiDpi::GetDpiForWindow,
    UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongPtrW, SetWindowLongW, SetWindowPos, GWLP_HWNDPARENT,
        GWL_EXSTYLE, GWL_STYLE, HTTRANSPARENT, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WM_NCDESTROY,
        WM_NCHITTEST, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_SYSMENU, WS_THICKFRAME,
    },
};

const SUBCLASS_ID: usize = 0x434C_5043;

pub struct Overlay {
    hwnd: isize,
}

impl Overlay {
    /// `owner` is the flyout's HWND; `overlay` is this window's HWND. Both are
    /// live GPUI windows on the calling UI thread.
    pub fn install(owner: isize, overlay: isize) -> Result<std::rc::Rc<Self>, &'static str> {
        if owner == 0 || overlay == 0 {
            return Err("missing connector overlay window handle");
        }
        unsafe {
            // Stack above the flyout (and transitively the main window) while
            // staying out of activation and the taskbar.
            SetWindowLongPtrW(overlay as HWND, GWLP_HWNDPARENT, owner);
            let style = GetWindowLongW(overlay as HWND, GWL_EXSTYLE)
                | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as i32;
            SetWindowLongW(overlay as HWND, GWL_EXSTYLE, style);
            // The overlay is a pure click-through canvas: it must carry no
            // system frame at all. Windows 11 otherwise rounds its corners and
            // strokes a 1px border (plus a drop shadow) around the whole
            // card∪flyout region, which reads as a large transparent box.
            let window_style = GetWindowLongW(overlay as HWND, GWL_STYLE)
                & !(WS_THICKFRAME | WS_CAPTION | WS_DLGFRAME | WS_BORDER | WS_SYSMENU) as i32;
            SetWindowLongW(overlay as HWND, GWL_STYLE, window_style);
            let attr_size = std::mem::size_of::<u32>() as u32;
            let no_round = DWMWCP_DONOTROUND as u32;
            DwmSetWindowAttribute(
                overlay as HWND,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &no_round as *const _ as _,
                attr_size,
            );
            let no_border = DWMWA_COLOR_NONE;
            DwmSetWindowAttribute(
                overlay as HWND,
                DWMWA_BORDER_COLOR as u32,
                &no_border as *const _ as _,
                attr_size,
            );
            let no_nc = DWMNCRP_DISABLED as u32;
            DwmSetWindowAttribute(
                overlay as HWND,
                DWMWA_NCRENDERING_POLICY as u32,
                &no_nc as *const _ as _,
                attr_size,
            );
            if SetWindowSubclass(overlay as HWND, Some(window_proc), SUBCLASS_ID, 0) == 0 {
                return Err("could not install connector overlay policy");
            }
            if SetWindowPos(
                overlay as HWND,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_HIDEWINDOW
                    | SWP_NOACTIVATE
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOOWNERZORDER
                    | SWP_NOZORDER,
            ) == 0
            {
                return Err("could not initialize connector overlay window");
            }
        }
        Ok(std::rc::Rc::new(Self { hwnd: overlay }))
    }

    /// Move, resize and show at a logical-screen rectangle.
    pub fn place(&self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let scale = (unsafe { GetDpiForWindow(self.hwnd as HWND) } as f32 / 96.).max(0.5);
        let to_physical = |value: f32| (value * scale).round() as i32;
        unsafe {
            SetWindowPos(
                self.hwnd as HWND,
                std::ptr::null_mut(),
                to_physical(x),
                to_physical(y),
                to_physical(width),
                to_physical(height),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER | SWP_SHOWWINDOW,
            ) != 0
        }
    }

    pub fn hide(&self) {
        unsafe {
            SetWindowPos(
                self.hwnd as HWND,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW
                    | SWP_NOACTIVATE
                    | SWP_NOOWNERZORDER
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER,
            );
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match message {
        // The overlay paints a line; every pixel passes input to the windows
        // below (same thread), so the list keeps receiving hover events.
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_NCDESTROY => unsafe {
            RemoveWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID);
            DefSubclassProc(hwnd, message, wparam, lparam)
        },
        // SAFETY: all remaining native behavior belongs to GPUI/Windows.
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowLongW, IsWindowVisible, SendMessageW, GWL_EXSTYLE,
        WS_POPUP,
    };

    struct TestWindow(HWND);

    impl TestWindow {
        fn new(owner: HWND) -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: built-in window class, no callbacks; off-screen and
            // initially hidden so this never opens an application.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    -32000,
                    -32000,
                    1,
                    1,
                    owner,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(!hwnd.is_null(), "create native test window");
            Self(hwnd)
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            // SAFETY: created on this test thread and destroyed exactly once.
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn missing_handles_cannot_install() {
        assert!(Overlay::install(0, 1).is_err());
        assert!(Overlay::install(1, 0).is_err());
    }

    #[test]
    fn hit_test_is_transparent_and_styles_are_applied() {
        let owner = TestWindow::new(std::ptr::null_mut());
        let overlay = TestWindow::new(owner.0);
        let policy = Overlay::install(owner.0 as isize, overlay.0 as isize)
            .expect("install connector overlay");

        // SAFETY: queries to our own live windows on this thread.
        unsafe {
            assert_eq!(
                SendMessageW(overlay.0, WM_NCHITTEST, 0, 0),
                HTTRANSPARENT as isize
            );
            let style = GetWindowLongW(overlay.0, GWL_EXSTYLE);
            assert_ne!(style & WS_EX_NOACTIVATE as i32, 0);
            assert_ne!(style & WS_EX_TOOLWINDOW as i32, 0);
            assert!(policy.place(-32000., -32000., 4., 4.));
            assert_ne!(IsWindowVisible(overlay.0), 0);
        }
        policy.hide();
        // SAFETY: read-only visibility check after hide.
        unsafe {
            assert_eq!(IsWindowVisible(overlay.0), 0);
        }
    }
}
