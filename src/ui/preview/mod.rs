//! Read-only clipboard preview: a native hover flyout beside the main window
//! and an optional standalone preview window.
//!
//! The two surfaces share [`content::PreviewModel`] so classification,
//! truncation limits and metadata never drift apart.

use gpui::{point, px, size, Bounds, Entity, Pixels, Size};

pub mod connector;
pub mod connector_overlay;
pub mod content;
pub mod flyout;
pub mod panel;
pub mod window;

pub use connector_overlay::PreviewConnectorView;
pub use flyout::PreviewFlyoutView;
pub use panel::{PreviewPanelEvent, PreviewPanelView};
pub use window::{PreviewWindowEvent, PreviewWindowView};

/// Docking target for the compact hover flyout: emitted by the clipboard
/// list, consumed by the WindowManager.
///
/// `card_top` is the hovered card's top edge in the main window's content
/// space; `card` and `pointer` are in logical **screen** pixels (the list
/// adds the main window's origin) so the connector overlay can use them
/// directly.
#[derive(Clone)]
pub struct PreviewFlyoutTarget {
    pub panel: Entity<PreviewPanelView>,
    pub card_top: f32,
    pub size: Size<Pixels>,
    pub prefer_right: bool,
    /// Hovered card rect in logical screen coordinates.
    pub card: Bounds<Pixels>,
    /// Pointer Y in the main window's content space (the same space as
    /// `card_top`). The flyout's vertical dock follows it, and the connector
    /// source anchor rides the flyout's center line.
    pub pointer_y: f32,
}

/// Gap between the main window and the docked standalone preview.
pub const PREVIEW_DOCK_GAP: f32 = 12.;

/// Dock the standalone preview next to the main window: prefer the right
/// side, flip to the left when the right side cannot hold the minimum width,
/// and keep the two top edges aligned. When neither side fits, the preview
/// still hugs the roomier side instead of covering the main window's center.
///
/// All coordinates and sizes are logical pixels. `work` is the work area of
/// the display that contains the main window.
pub fn dock_preview_bounds(
    main: Bounds<Pixels>,
    work: Bounds<Pixels>,
    preferred: Size<Pixels>,
    min: Size<Pixels>,
) -> Bounds<Pixels> {
    let gap = px(PREVIEW_DOCK_GAP);
    let min_width = min.width.min(work.size.width);
    let preferred_width = preferred.width.min(work.size.width).max(min_width);
    let min_height = min.height.min(work.size.height);
    let height = preferred.height.min(work.size.height).max(min_height);

    let right_space = (work.right() - main.right() - gap).max(px(0.));
    let left_space = (main.left() - work.left() - gap).max(px(0.));

    let (on_right, width) = if right_space >= preferred_width {
        (true, preferred_width)
    } else if left_space >= preferred_width {
        (false, preferred_width)
    } else if right_space >= min_width && right_space >= left_space {
        (true, right_space.min(preferred_width))
    } else if left_space >= min_width {
        (false, left_space.min(preferred_width))
    } else {
        // Neither side can hold the minimum window. Stay on the roomier
        // side and let the window overlap the main one; never center it.
        (right_space >= left_space, min_width)
    };

    let raw_x = if on_right {
        main.right() + gap
    } else {
        main.left() - gap - width
    };
    let max_x = (work.right() - width).max(work.left());
    let x = raw_x.clamp(work.left(), max_x);
    let max_y = (work.bottom() - height).max(work.top());
    let y = main.top().clamp(work.top(), max_y);

    Bounds::new(point(x, y), size(width, height))
}

#[cfg(test)]
mod tests {
    use super::{dock_preview_bounds, PREVIEW_DOCK_GAP};
    use gpui::{point, px, size, Bounds, Pixels};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn dock(main: Bounds<Pixels>, work: Bounds<Pixels>) -> Bounds<Pixels> {
        dock_preview_bounds(
            main,
            work,
            size(px(760.), px(560.)),
            size(px(480.), px(360.)),
        )
    }

    #[test]
    fn prefers_the_right_side_and_aligns_the_top_edges() {
        let rect = dock(rect(100., 100., 324., 600.), rect(0., 0., 1920., 1080.));
        assert_eq!(rect.left(), px(100. + 324. + PREVIEW_DOCK_GAP));
        assert_eq!(rect.top(), px(100.));
        assert_eq!(rect.size, size(px(760.), px(560.)));
    }

    #[test]
    fn flips_to_the_left_when_the_right_side_is_crowded() {
        let rect = dock(rect(1200., 100., 324., 600.), rect(0., 0., 1920., 1080.));
        assert_eq!(rect.left(), px(1200. - PREVIEW_DOCK_GAP - 760.));
        assert_eq!(rect.top(), px(100.));
    }

    #[test]
    fn shrinks_to_the_available_left_side() {
        // Right has 364px (< 480), left has 488px (>= 480 but < 760).
        let rect = dock(rect(500., 100., 324., 600.), rect(0., 0., 1200., 800.));
        assert_eq!(rect.left(), px(0.));
        assert_eq!(rect.size.width, px(488.));
    }

    #[test]
    fn never_centers_when_nothing_fits() {
        // Maximized main window: both sides have zero room.
        let rect = dock(rect(0., 0., 1920., 1080.), rect(0., 0., 1920., 1080.));
        assert_eq!(rect.size.width, px(480.));
        assert_eq!(rect.left(), px(1920. - 480.));
        assert_eq!(rect.top(), px(0.));
    }

    #[test]
    fn keeps_the_top_edge_on_screen() {
        let rect = dock(rect(100., 800., 324., 280.), rect(0., 0., 1920., 1080.));
        assert_eq!(rect.top(), px(1080. - 560.));
    }

    #[test]
    fn fits_a_small_work_area() {
        let rect = dock(rect(0., 0., 400., 300.), rect(0., 0., 400., 300.));
        assert_eq!(rect.size, size(px(400.), px(300.)));
        assert_eq!(rect.origin, point(px(0.), px(0.)));
    }
}
