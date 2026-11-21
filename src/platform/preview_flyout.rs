//! Native hover-preview flyout.
//!
//! The flyout is a non-activating popup owned by the main window. It never
//! becomes the key window, so hovering and clicking it cannot steal focus from
//! the main search field. [`dock_flyout_bounds`] is the shared placement math;
//! the platform modules only supply screen geometry and move the native window.

#[cfg(target_os = "windows")]
#[path = "preview_flyout_windows.rs"]
mod implementation;
#[cfg(target_os = "macos")]
#[path = "preview_flyout_macos.rs"]
mod implementation;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use implementation::Attachment;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use implementation::LogicalRect;

/// Gap between the main window and the flyout, in logical pixels.
pub const FLYOUT_GAP: f32 = 12.;

/// Raw top edge for a flyout centered on the source anchor's center line.
/// Callers pass the 1px pointer anchor's top and height (see
/// `ui::preview::connector::pointer_anchor_rect`), so the dock follows the
/// pointer's Y within the hovered card. Clamping to the work area happens in
/// [`dock_flyout_bounds`].
pub fn centered_dock_top(card_top: f32, card_height: f32, panel_height: f32) -> f32 {
    card_top + card_height / 2. - panel_height / 2.
}

/// Place the flyout beside the main window.
///
/// `main` and `work` are `(x, y, width, height)` in logical screen pixels;
/// `anchor_y` is the flyout's desired raw top edge in the same coordinate
/// space (callers center it on the pointer anchor via [`centered_dock_top`]).
///
/// The width is decided **only by the room around the main window**, never by
/// the pointer: the flyout sits on the right whenever the right side can hold
/// it, and only flips to the left when it cannot. Following the pointer would
/// slide the sheet sideways as the cursor crosses the card, which reads as the
/// preview wandering instead of docking. When neither side fits, the roomier
/// side still wins and the flyout may overlap the main window — it must never
/// be centered.
pub fn dock_flyout_bounds(
    main: (f32, f32, f32, f32),
    anchor_y: f32,
    work: (f32, f32, f32, f32),
    size: (f32, f32),
) -> (f32, f32, f32, f32) {
    let (main_x, _, main_width, _) = main;
    let (work_x, work_y, work_width, work_height) = work;
    let main_right = main_x + main_width;
    let work_right = work_x + work_width.max(1.);
    let work_bottom = work_y + work_height.max(1.);

    let width = size.0.min(work_width.max(1.));
    let height = size.1.min(work_height.max(1.));
    let right_space = (work_right - main_right - FLYOUT_GAP).max(0.);
    let left_space = (main_x - work_x - FLYOUT_GAP).max(0.);
    let fits_right = right_space >= width;
    let fits_left = left_space >= width;

    let on_right = match (fits_right, fits_left) {
        (true, _) => true,
        (false, true) => false,
        // Neither side can hold the sheet: keep the roomier one.
        (false, false) => right_space >= left_space,
    };

    let raw_x = if on_right {
        main_right + FLYOUT_GAP
    } else {
        main_x - FLYOUT_GAP - width
    };
    let x = raw_x.clamp(work_x, (work_right - width).max(work_x));
    let y = anchor_y.clamp(work_y, (work_bottom - height).max(work_y));

    (x, y, width, height)
}

/// Logical screen rectangle: `(x, y, width, height)`.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub type LogicalRect = (f32, f32, f32, f32);

/// Non-Windows/macOS builds have no native flyout; the caller simply keeps the
/// preview hidden instead of failing to compile.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct Attachment;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Attachment {
    pub fn install(_main: isize, _flyout: isize) -> Result<std::rc::Rc<Self>, &'static str> {
        Err("native preview flyout is unavailable on this platform")
    }

    pub fn docked_rect(
        &self,
        _card_top: f32,
        _card_height: f32,
        _width: f32,
        _height: f32,
    ) -> Option<LogicalRect> {
        None
    }

    pub fn expand_rect(&self, _width: f32, _height: f32) -> Option<LogicalRect> {
        None
    }

    pub fn work_area(&self) -> Option<LogicalRect> {
        None
    }

    pub fn current_rect(&self) -> Option<LogicalRect> {
        None
    }

    pub fn canvas_origin(&self) -> Option<(f32, f32)> {
        None
    }

    pub fn place(&self, _rect: LogicalRect) -> bool {
        false
    }

    pub fn is_shown(&self) -> bool {
        false
    }

    pub fn hide(&self) {}
}

#[cfg(test)]
mod tests {
    use super::{centered_dock_top, dock_flyout_bounds, FLYOUT_GAP};

    const WORK: (f32, f32, f32, f32) = (0., 0., 1920., 1080.);
    const SIZE: (f32, f32) = (340., 300.);

    #[test]
    fn centered_dock_top_aligns_the_center_lines() {
        // Card 40 tall at y=200 (center 220); panel 300 tall → top 70.
        assert_eq!(centered_dock_top(200., 40., 300.), 70.);
        // Same-height card and panel keep the top edge.
        assert_eq!(centered_dock_top(200., 300., 300.), 200.);
    }

    #[test]
    fn stays_on_the_right_when_both_sides_fit() {
        // 两侧都有余量时固定贴右侧：位置不能随指针左右漂移。
        let main = (800., 100., 324., 600.);
        let bounds = dock_flyout_bounds(main, 120., WORK, SIZE);
        assert_eq!(bounds.0, 800. + 324. + FLYOUT_GAP);
        assert_eq!(bounds.1, 120.);
    }

    #[test]
    fn flips_to_the_side_with_room() {
        // Right of the main window has only 84px left.
        let main = (1500., 100., 324., 600.);
        let bounds = dock_flyout_bounds(main, 120., WORK, SIZE);
        assert_eq!(bounds.0, 1500. - FLYOUT_GAP - 340.);
        assert_eq!(bounds.2, 340.);
    }

    #[test]
    fn never_centers_when_neither_side_fits() {
        // Maximized main window: no outside room on either side.
        let main = (0., 0., 1920., 1080.);
        let bounds = dock_flyout_bounds(main, 400., WORK, SIZE);
        assert_eq!(bounds.0, 1920. - 340.);
        assert_eq!(bounds.1, 400.);
    }

    #[test]
    fn clamps_the_bottom_edge() {
        let main = (100., 700., 324., 380.);
        let bounds = dock_flyout_bounds(main, 1000., WORK, SIZE);
        assert_eq!(bounds.1, 1080. - 300.);
    }

    #[test]
    fn fits_a_small_work_area() {
        let work = (0., 0., 300., 200.);
        let bounds = dock_flyout_bounds((0., 0., 300., 200.), 50., work, SIZE);
        assert_eq!(bounds, (0., 0., 300., 200.));
    }
}
