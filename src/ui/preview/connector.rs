//! Connector geometry between the hovered list card and the preview flyout.
//!
//! Cubic Bezier with edge-midpoint anchors, ported from EcoPaste
//! `src/pages/Preview/geometry.ts` (`resolveConnector`): the two rects'
//! facing edge midpoints become the anchor points, control points project
//! along the edge normals at a distance clamped to 20–64 px. The flyout is
//! docked centered on the card's center line (EcoPaste `rawDynamicPanelRect`),
//! so both anchors share a height and the curve reads as a straight line.
//!
//! All coordinates are logical screen pixels.

/// Logical screen rectangle: `(x, y, width, height)`.
pub type ScreenRect = (f32, f32, f32, f32);

/// Logical screen point: `(x, y)`.
pub type ScreenPoint = (f32, f32);

const CONTROL_RATIO: f32 = 0.22;
const MIN_CONTROL_DISTANCE: f32 = 20.;
const MAX_CONTROL_DISTANCE: f32 = 64.;

/// Padding around the card/panel union when sizing the overlay window: the
/// bezier's convex hull (control points) may bulge up to 64 px past an anchor.
pub const OVERLAY_PAD: f32 = 72.;

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
    let control_distance =
        (distance * CONTROL_RATIO).clamp(MIN_CONTROL_DISTANCE, MAX_CONTROL_DISTANCE);
    ConnectorGeometry {
        source: source.point,
        target: target.point,
        control1: project(source.point, source_direction, control_distance),
        control2: project(target.point, target_direction, control_distance),
    }
}

/// Linear interpolation between two connector geometries (retarget motion).
/// The easing itself belongs to `design::motion`.
pub fn mix_geometry(from: &ConnectorGeometry, to: &ConnectorGeometry, t: f32) -> ConnectorGeometry {
    let t = t.clamp(0., 1.);
    let mix_point = |a: ScreenPoint, b: ScreenPoint| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    ConnectorGeometry {
        source: mix_point(from.source, to.source),
        target: mix_point(from.target, to.target),
        control1: mix_point(from.control1, to.control1),
        control2: mix_point(from.control2, to.control2),
    }
}

/// Interpolate a screen rect (window-rect animation; easing from
/// `design::motion`).
pub fn lerp_rect(from: ScreenRect, to: ScreenRect, t: f32) -> ScreenRect {
    let t = t.clamp(0., 1.);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    (
        lerp(from.0, to.0),
        lerp(from.1, to.1),
        lerp(from.2, to.2),
        lerp(from.3, to.3),
    )
}

/// Bounding box of the given rects, inflated by `pad` on every side.
pub fn padded_union(rects: &[ScreenRect], pad: f32) -> ScreenRect {
    let Some(first) = rects.first() else {
        return (0., 0., 0., 0.);
    };
    let (mut left, mut top, mut right, mut bottom) =
        (first.0, first.1, first.0 + first.2, first.1 + first.3);
    for rect in &rects[1..] {
        left = left.min(rect.0);
        top = top.min(rect.1);
        right = right.max(rect.0 + rect.2);
        bottom = bottom.max(rect.1 + rect.3);
    }
    (
        left - pad,
        top - pad,
        (right + pad - (left - pad)).max(0.),
        (bottom + pad - (top - pad)).max(0.),
    )
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
        // Very close rects: distance * 0.22 < 20, so both controls sit at 20.
        let geometry = resolve_connector(rect(0., 0., 40., 10.), rect(60., 0., 40., 10.));
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
    fn mix_geometry_interpolates_between_two_connectors() {
        let a = resolve_connector(rect(0., 0., 40., 10.), rect(100., 0., 40., 10.));
        let b = resolve_connector(rect(0., 60., 40., 10.), rect(100., 60., 40., 10.));
        // a: card at y=0 (edge mid y=5); b: card at y=60 (edge mid y=65).
        assert!((mix_geometry(&a, &b, 0.5).source.1 - 35.).abs() < 1e-3);
        assert!((mix_geometry(&a, &b, 0.5).target.1 - 35.).abs() < 1e-3);
        assert_eq!(mix_geometry(&a, &b, 0.), a);
        assert_eq!(mix_geometry(&a, &b, 1.), b);
    }

    #[test]
    fn lerp_and_padded_union_cover_their_inputs() {
        assert_eq!(
            lerp_rect((0., 0., 10., 10.), (20., 40., 30., 50.), 0.5),
            (10., 20., 20., 30.)
        );
        assert_eq!(
            lerp_rect((0., 0., 10., 10.), (20., 40., 30., 50.), 1.),
            (20., 40., 30., 50.)
        );

        let union = padded_union(&[(100., 100., 50., 40.), (0., 300., 10., 10.)], 8.);
        assert_eq!(
            union,
            (-8., 92., 100. + 50. + 8. + 8., 300. + 10. + 8. - 92.)
        );
        assert_eq!(padded_union(&[], 8.), (0., 0., 0., 0.));
    }

    #[test]
    fn overlapping_rects_still_resolve_without_panicking() {
        let geometry = resolve_connector(rect(0., 0., 100., 100.), rect(50., 50., 100., 100.));
        assert!(geometry.source.0.is_finite());
        assert!(geometry.target.0.is_finite());
    }
}
