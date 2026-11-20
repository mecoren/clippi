use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
            DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        },
        Gdi::ClientToScreen,
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowLongW, GetWindowRect, IsWindowVisible, SetWindowLongPtrW, SetWindowLongW,
            SetWindowPos, GWLP_HWNDPARENT, GWL_STYLE, HTCLIENT, HTTRANSPARENT, SWP_FRAMECHANGED,
            SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
            SWP_NOZORDER, WM_NCDESTROY, WM_NCHITTEST, WS_BORDER, WS_CAPTION, WS_DLGFRAME,
            WS_SYSMENU, WS_THICKFRAME,
        },
    },
};

use super::dock_flyout_bounds;
use crate::platform::{monitor, quick_window};

type ScreenRect = (i32, i32, i32, i32);

/// Logical screen rectangle: `(x, y, width, height)`.
pub type LogicalRect = (f32, f32, f32, f32);

const HIT_SUBCLASS_ID: usize = 0x434C_5051;

thread_local! {
    /// Card hit rect per flyout HWND, in client pixels. The native canvas spans
    /// the whole work area and never resizes while shown; only the card inside
    /// it may receive input, everything else passes through to the windows below.
    static CARD_HIT: RefCell<HashMap<isize, ScreenRect>> = RefCell::new(HashMap::new());
}

pub struct Attachment {
    main: isize,
    flyout: isize,
    canvas: Cell<Option<ScreenRect>>,
    card: Cell<Option<ScreenRect>>,
}

impl Attachment {
    /// Both handles are live GPUI windows on the calling UI thread.
    pub fn install(main: isize, flyout: isize) -> Result<Rc<Self>, &'static str> {
        if main == 0 || flyout == 0 {
            return Err("missing preview flyout window handle");
        }
        // Ownership keeps the popup above the main window while the main is
        // foreground, without making it topmost over other applications.
        unsafe {
            SetWindowLongPtrW(flyout as HWND, GWLP_HWNDPARENT, main);
        }
        quick_window::install_owned(flyout)?;
        // The card frame, stroke and shadow are drawn in-app so they move with
        // the rendered content. DWM's native frame/shadow instead track the live
        // window rect, which updates a composite ahead of the content during the
        // grow/shrink animation and reads as edge jitter; strip it entirely.
        unsafe {
            let window_style = GetWindowLongW(flyout as HWND, GWL_STYLE)
                & !(WS_THICKFRAME | WS_CAPTION | WS_DLGFRAME | WS_BORDER | WS_SYSMENU) as i32;
            SetWindowLongW(flyout as HWND, GWL_STYLE, window_style);
            let attr_size = std::mem::size_of::<u32>() as u32;
            let no_round = DWMWCP_DONOTROUND as u32;
            DwmSetWindowAttribute(
                flyout as HWND,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &no_round as *const _ as _,
                attr_size,
            );
            let no_border = DWMWA_COLOR_NONE;
            DwmSetWindowAttribute(
                flyout as HWND,
                DWMWA_BORDER_COLOR as u32,
                &no_border as *const _ as _,
                attr_size,
            );
            let no_nc = DWMNCRP_DISABLED as u32;
            DwmSetWindowAttribute(
                flyout as HWND,
                DWMWA_NCRENDERING_POLICY as u32,
                &no_nc as *const _ as _,
                attr_size,
            );
            SetWindowPos(
                flyout as HWND,
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
            );
            // Installed after the quick-window policy so this hit-test runs
            // first: the transparent canvas must not swallow pointer input
            // outside the card.
            if SetWindowSubclass(flyout as HWND, Some(hit_proc), HIT_SUBCLASS_ID, 0) == 0 {
                return Err("could not install preview flyout hit test");
            }
        }
        Ok(Rc::new(Self {
            main,
            flyout,
            canvas: Cell::new(None),
            card: Cell::new(None),
        }))
    }

    /// Docked compact rect for a source anchor, in logical screen pixels.
    /// `card_top`/`card_height` describe the 1px pointer anchor; the caller
    /// springs [`Attachment::place`] towards this target.
    pub fn docked_rect(
        &self,
        card_top: f32,
        card_height: f32,
        width: f32,
        height: f32,
        prefer_right: bool,
    ) -> Option<LogicalRect> {
        let (main, scale, work) = self.screen_context()?;
        let mut client_origin = POINT { x: 0, y: 0 };
        // SAFETY: read-only query on our own window.
        if unsafe { ClientToScreen(self.main as HWND, &mut client_origin) } == 0 {
            return None;
        }

        let main_logical = (
            main.0 as f32 / scale,
            main.1 as f32 / scale,
            (main.2 - main.0) as f32 / scale,
            (main.3 - main.1) as f32 / scale,
        );
        let anchor_y = client_origin.y as f32 / scale
            + super::centered_dock_top(card_top, card_height, height);
        let work_logical = (
            work.0 as f32 / scale,
            work.1 as f32 / scale,
            work.2 as f32 / scale,
            work.3 as f32 / scale,
        );

        Some(dock_flyout_bounds(
            main_logical,
            anchor_y,
            work_logical,
            (width, height),
            prefer_right,
        ))
    }

    pub fn hide(&self) {
        self.card.set(None);
        self.canvas.set(None);
        CARD_HIT.with(|hits| {
            hits.borrow_mut().remove(&self.flyout);
        });
        quick_window::hide(self.flyout);
    }

    /// Expanded rect for the full preview, grown from the current card:
    /// a right-side sheet keeps its left edge, a left-side sheet keeps its
    /// right edge; growth stays centered on the card's center line so the
    /// connector remains a straight line, clamped to the work area.
    pub fn expand_rect(&self, width: f32, height: f32) -> Option<LogicalRect> {
        let (main, scale, work) = self.screen_context()?;
        let compact_logical = self.current_rect()?;
        let work_logical = (
            work.0 as f32 / scale,
            work.1 as f32 / scale,
            work.2 as f32 / scale,
            work.3 as f32 / scale,
        );
        let main_center = (main.0 + main.2) as f32 / 2. / scale;
        let on_right = compact_logical.0 + compact_logical.2 / 2. >= main_center;
        let raw_x = if on_right {
            compact_logical.0
        } else {
            compact_logical.0 + compact_logical.2 - width
        };
        let x = raw_x.clamp(
            work_logical.0,
            (work_logical.0 + work_logical.2 - width).max(work_logical.0),
        );
        let y = (compact_logical.1 + compact_logical.3 / 2. - height / 2.).clamp(
            work_logical.1,
            (work_logical.1 + work_logical.3 - height).max(work_logical.1),
        );
        Some((x, y, width, height))
    }

    /// The card's current rect in logical screen pixels. The native window is
    /// a fixed canvas, so the animated rect lives here, not in the window rect.
    pub fn current_rect(&self) -> Option<LogicalRect> {
        let card = self.card.get()?;
        let scale = self.scale();
        Some((
            card.0 as f32 / scale,
            card.1 as f32 / scale,
            card.2 as f32 / scale,
            card.3 as f32 / scale,
        ))
    }

    /// Logical screen origin of the fixed native canvas. The card is drawn at
    /// `card - canvas_origin` inside the GPUI window.
    pub fn canvas_origin(&self) -> Option<(f32, f32)> {
        let canvas = self.canvas.get()?;
        let scale = self.scale();
        Some((canvas.0 as f32 / scale, canvas.1 as f32 / scale))
    }

    /// Show the card at a logical-screen rectangle. The native canvas is created
    /// once at the work area and never resized afterwards: resizing a native
    /// window per animation frame composites a stale swapchain against the new
    /// rect and reads as jitter. Only the in-app card rect moves.
    pub fn place(&self, rect: LogicalRect) -> bool {
        let Some((_, scale, work)) = self.screen_context() else {
            return false;
        };
        let canvas = (work.0, work.1, work.2, work.3);
        if self.canvas.get() != Some(canvas) {
            if !quick_window::show_at_owned(self.flyout, canvas.0, canvas.1, canvas.2, canvas.3) {
                return false;
            }
            self.canvas.set(Some(canvas));
        }
        let to_physical = |value: f32| (value * scale).round() as i32;
        let card = (
            to_physical(rect.0),
            to_physical(rect.1),
            to_physical(rect.2),
            to_physical(rect.3),
        );
        self.card.set(Some(card));
        let hit = (card.0 - canvas.0, card.1 - canvas.1, card.2, card.3);
        CARD_HIT.with(|hits| {
            hits.borrow_mut().insert(self.flyout, hit);
        });
        true
    }

    /// Work area of the main window's monitor in logical screen pixels. The
    /// connector overlay covers it once instead of chasing every retarget.
    pub fn work_area(&self) -> Option<LogicalRect> {
        let (_, scale, work) = self.screen_context()?;
        Some((
            work.0 as f32 / scale,
            work.1 as f32 / scale,
            work.2 as f32 / scale,
            work.3 as f32 / scale,
        ))
    }

    /// Whether the flyout window is currently visible.
    pub fn is_shown(&self) -> bool {
        // SAFETY: read-only query on our own window.
        unsafe { IsWindowVisible(self.flyout as HWND) != 0 }
    }

    fn scale(&self) -> f32 {
        (unsafe { GetDpiForWindow(self.main as HWND) } as f32 / 96.).max(0.5)
    }

    /// Main outer rect, DPI scale and work area, all in physical pixels.
    fn screen_context(&self) -> Option<(ScreenRect, f32, ScreenRect)> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: read-only queries on our own windows.
        if unsafe { GetWindowRect(self.main as HWND, &mut rect) } == 0 {
            return None;
        }
        let scale = self.scale();
        let work = monitor::get_monitor_work_area(rect.left, rect.top)?;
        Some((
            (rect.left, rect.top, rect.right, rect.bottom),
            scale,
            (work.x, work.y, work.width, work.height),
        ))
    }
}

/// Whether a client-space point falls inside the card hit rect.
fn card_contains(hit: ScreenRect, x: i32, y: i32) -> bool {
    x >= hit.0 && x < hit.0 + hit.2 && y >= hit.1 && y < hit.1 + hit.3
}

unsafe extern "system" fn hit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            let x = lparam as i16 as i32;
            let y = (lparam >> 16) as i16 as i32;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: read-only query on our own window.
            if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                return HTTRANSPARENT as LRESULT;
            }
            let (rx, ry) = (x - rect.left, y - rect.top);
            let inside = CARD_HIT
                .with(|hits| hits.borrow().get(&(hwnd as isize)).copied())
                .is_some_and(|r| card_contains(r, rx, ry));
            if inside {
                HTCLIENT as LRESULT
            } else {
                HTTRANSPARENT as LRESULT
            }
        }
        WM_NCDESTROY => {
            CARD_HIT.with(|hits| {
                hits.borrow_mut().remove(&(hwnd as isize));
            });
            // SAFETY: remove only our subclass and forward final destruction.
            unsafe {
                RemoveWindowSubclass(hwnd, Some(hit_proc), HIT_SUBCLASS_ID);
                DefSubclassProc(hwnd, message, wparam, lparam)
            }
        }
        // SAFETY: all remaining native behavior belongs to GPUI/Windows.
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_hit_rect_includes_edges_but_not_the_outside() {
        let hit = (100, 200, 340, 300);
        assert!(card_contains(hit, 100, 200));
        assert!(card_contains(hit, 439, 499));
        assert!(!card_contains(hit, 440, 499));
        assert!(!card_contains(hit, 439, 500));
        assert!(!card_contains(hit, 99, 200));
        assert!(!card_contains(hit, 100, 199));
    }

    #[test]
    fn empty_hit_rect_never_contains() {
        assert!(!card_contains((0, 0, 0, 0), 0, 0));
    }
}
