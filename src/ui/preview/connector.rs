//! Connector geometry between the hovered list card and the preview flyout.
//!
//! Cubic Bezier with edge-midpoint anchors: the two rects' facing edge
//! midpoints become the anchor points, control points project along the edge
//! normals at a distance clamped to 20–64 px (and never past the anchors'
//! midpoint). The source rect is a 1px tall anchor clamped inside the card
//! (see [`pointer_anchor_rect`]) riding the flyout's center line; the flyout
//! in turn docks to the pointer, so the line tracks the pointer vertically
//! and stays straight instead of jumping between card centers.
//!
//! All coordinates are logical screen pixels.

/// Logical screen rectangle: `(x, y, width, height)`.
pub type ScreenRect = (f32, f32, f32, f32);

/// Logical screen point: `(x, y)`.
pub type ScreenPoint = (f32, f32);

const CONTROL_RATIO: f32 = 0.22;
const MIN_CONTROL_DISTANCE: f32 = 20.;
const MAX_CONTROL_DISTANCE: f32 = 64.;

/// The four points defining the card → panel connector curve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConnectorGeometry {
    pub source: ScreenPoint,
    pub target: ScreenPoint,
    pub control1: ScreenPoint,
    pub control2: ScreenPoint,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy)]
struct Anchor {
    point: ScreenPoint,
    side: Side,
}

/// Horizontal padding around the connector's x-span when sizing the overlay
/// window: the bezier's control points may bulge up to 64 px past an anchor.
pub const OVERLAY_PAD: f32 = 72.;

/// Height of the pointer-anchored source rect, in logical pixels.
pub const POINTER_ANCHOR_SIZE: f32 = 1.;

/// Source anchor aligned with the panel's current center line. Unlike the
/// dock anchor this is deliberately NOT clamped into the card: both ends
/// share the panel's height on every frame, so the line stays straight and
/// the two ends arrive together while the panel springs. Clamping would pin
/// the source to the next card's edge before the panel gets there.
pub fn panel_aligned_source(card: ScreenRect, panel: ScreenRect) -> ScreenRect {
    (
        card.0,
        panel.1 + panel.3 / 2. - POINTER_ANCHOR_SIZE / 2.,
        card.2,
        POINTER_ANCHOR_SIZE,
    )
}

/// Collapse the hovered card into a 1px tall source rect centered on `anchor_y`
/// (the pointer's Y, or the flyout's current center line), clamped to the
/// card's vertical span. The horizontal extent stays the card's, so the
/// connector still leaves from the card's side edge.
pub fn pointer_anchor_rect(card: ScreenRect, pointer_y: f32) -> ScreenRect {
    let center_y = pointer_y.clamp(card.1, card.1 + card.3);
    (
        card.0,
        center_y - POINTER_ANCHOR_SIZE / 2.,
        card.2,
        POINTER_ANCHOR_SIZE,
    )
}

/// Resolve the connector curve between `source_rect` and `target_rect`.
pub fn resolve_connector(source_rect: ScreenRect, target_rect: ScreenRect) -> ConnectorGeometry {
    let mut best: Option<(f32, Anchor, Anchor)> = None;
    for source in anchor_candidates(source_rect) {
        for target in anchor_candidates(target_rect) {
            let score = score_candidate(&source, &target);
            if best.is_none_or(|(current, _, _)| score < current) {
                best = Some((score, source, target));
            }
        }
    }
    let (_, source, target) = best.expect("anchor candidates are never empty");
    let source_direction = normal_for_side(source.side);
    let target_direction = normal_for_side(target.side);
    let distance = point_distance(source.point, target.point);
    // Never let the control points cross the midpoint between the anchors:
    // the dock gap is narrower than MIN_CONTROL_DISTANCE, so an unclamped
    // projection would loop the curve back on itself whenever the two ends
    // sit at different heights.
    let axis_gap = if source_direction.0 != 0. {
        (target.point.0 - source.point.0).abs()
    } else {
        (target.point.1 - source.point.1).abs()
    };
    let control_distance = (distance * CONTROL_RATIO)
        .clamp(MIN_CONTROL_DISTANCE, MAX_CONTROL_DISTANCE)
        .min(axis_gap * 0.5);
    ConnectorGeometry {
        source: source.point,
        target: target.point,
        control1: project(source.point, source_direction, control_distance),
        control2: project(target.point, target_direction, control_distance),
    }
}

fn anchor_candidates(rect: ScreenRect) -> [Anchor; 4] {
    let (left, top, width, height) = rect;
    let (right, bottom) = (left + width, top + height);
    let center_x = left + width / 2.;
    let center_y = top + height / 2.;
    [
        Anchor {
            point: (left, center_y),
            side: Side::Left,
        },
        Anchor {
            point: (right, center_y),
            side: Side::Right,
        },
        Anchor {
            point: (center_x, top),
            side: Side::Top,
        },
        Anchor {
            point: (center_x, bottom),
            side: Side::Bottom,
        },
    ]
}

/// Prefer short connections whose anchor normals point at each other; a
/// misaligned normal adds a flat 240 px penalty (EcoPaste `scoreCandidate`).
fn score_candidate(source: &Anchor, target: &Anchor) -> f32 {
    let direction = (
        target.point.0 - source.point.0,
        target.point.1 - source.point.1,
    );
    let distance = point_distance(source.point, target.point);
    let source_penalty = if dot(normal_for_side(source.side), direction) > 0. {
        0.
    } else {
        240.
    };
    let target_penalty = if dot(normal_for_side(target.side), (-direction.0, -direction.1)) > 0. {
        0.
    } else {
        240.
    };
    distance + source_penalty + target_penalty
}

fn normal_for_side(side: Side) -> ScreenPoint {
    match side {
        Side::Left => (-1., 0.),
        Side::Right => (1., 0.),
        Side::Top => (0., -1.),
        Side::Bottom => (0., 1.),
    }
}

fn project(point: ScreenPoint, direction: ScreenPoint, distance: f32) -> ScreenPoint {
    (
        point.0 + direction.0 * distance,
        point.1 + direction.1 * distance,
    )
}

fn point_distance(a: ScreenPoint, b: ScreenPoint) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn dot(a: ScreenPoint, b: ScreenPoint) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ScreenRect {
        (x, y, w, h)
    }

    #[test]
    fn connects_the_facing_edges_of_two_beside_rects() {
        // Card on the left, panel on the right: right-edge → left-edge.
        let geometry = resolve_connector(rect(100., 200., 200., 40.), rect(600., 180., 300., 200.));
        assert_eq!(geometry.source, (300., 220.));
        assert_eq!(geometry.target, (600., 280.));
        // Control points extend along the outward normals.
        assert!(geometry.control1.0 > geometry.source.0);
        assert!(geometry.control2.0 < geometry.target.0);
        assert_eq!(geometry.control1.1, geometry.source.1);
        assert_eq!(geometry.control2.1, geometry.target.1);
    }

    #[test]
    fn control_distance_is_clamped_to_the_e2e_bounds() {
        // Wide gap, short distance: distance * 0.22 < 20, raised to 20.
        let geometry = resolve_connector(rect(0., 0., 40., 10.), rect(100., 0., 40., 10.));
        let control_length = (geometry.control1.0 - geometry.source.0).abs();
        assert!(
            (control_length - 20.).abs() < 1e-3,
            "control length {control_length}"
        );
        // Very far rects: distance * 0.22 > 64, clamped to 64.
        let geometry = resolve_connector(rect(0., 0., 40., 10.), rect(2000., 0., 40., 10.));
        let control_length = (geometry.control1.0 - geometry.source.0).abs();
        assert!(
            (control_length - 64.).abs() < 1e-3,
            "control length {control_length}"
        );
    }

    #[test]
    fn control_points_never_cross_the_anchor_midpoint() {
        // A 20px gap cannot host two 20px control projections: both controls
        // stop at the midpoint instead of looping the curve back on itself.
        let geometry = resolve_connector(rect(0., 0., 40., 10.), rect(60., 0., 40., 10.));
        assert_eq!(geometry.source, (40., 5.));
        assert_eq!(geometry.target, (60., 5.));
        assert_eq!(geometry.control1.0, 50.);
        assert_eq!(geometry.control2.0, 50.);
    }

    #[test]
    fn centered_dock_yields_a_straight_horizontal_line() {
        let card = rect(100., 200., 200., 40.);
        let top = crate::platform::preview_flyout::centered_dock_top(200., 40., 200.);
        let geometry = resolve_connector(card, rect(600., top, 300., 200.));
        // Both edge midpoints share a height, and the controls stay on it:
        // the cubic degenerates to a straight segment.
        assert_eq!(geometry.source.1, geometry.target.1);
        assert_eq!(geometry.control1.1, geometry.source.1);
        assert_eq!(geometry.control2.1, geometry.target.1);
    }

    #[test]
    fn pointer_anchor_follows_the_pointer_inside_the_card() {
        let card = rect(100., 200., 200., 40.);
        let anchor = pointer_anchor_rect(card, 220.);
        assert_eq!(anchor, (100., 219.5, 200., 1.));
        // Clamped to the card's top and bottom edges.
        assert_eq!(pointer_anchor_rect(card, 0.).1, 199.5);
        assert_eq!(pointer_anchor_rect(card, 1000.).1, 239.5);
        // Horizontal extent is untouched.
        assert_eq!(pointer_anchor_rect(card, 220.).0, 100.);
        assert_eq!(pointer_anchor_rect(card, 220.).2, 200.);
    }

    #[test]
    fn panel_aligned_source_keeps_both_ends_level() {
        let card = rect(600., 200., 200., 40.);
        let panel = rect(100., 120., 300., 200.);
        let source = panel_aligned_source(card, panel);
        let geometry = resolve_connector(source, panel);
        // Both ends share the panel's center height: a straight line.
        assert_eq!(geometry.source.1, geometry.target.1);
        // Even when the panel's center leaves the card's span (work-area
        // clamp, or mid-retarget over another card) the source rides along
        // instead of snapping to the card's edge ahead of the panel.
        let far_panel = rect(100., 900., 300., 200.);
        let source = panel_aligned_source(card, far_panel);
        assert_eq!(source.1, 999.5);
        let geometry = resolve_connector(source, far_panel);
        assert_eq!(geometry.source.1, geometry.target.1);
    }

    #[test]
    fn overlapping_rects_still_resolve_without_panicking() {
        let geometry = resolve_connector(rect(0., 0., 100., 100.), rect(50., 50., 100., 100.));
        assert!(geometry.source.0.is_finite());
        assert!(geometry.target.0.is_finite());
    }
}
