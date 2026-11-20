//! Transparent click-through overlay that paints the card → flyout connector.
//!
//! The overlay is a separate native window owned by the flyout (see
//! `platform::preview_overlay`) covering the region between the hovered card
//! and the preview panel. It only paints the EcoPaste-style bezier line and
//! its two endpoint dots; input falls through to the windows underneath.
//!
//! Coordinates: everything the view receives is logical **screen** pixels;
//! `origin` (the overlay window's top-left) translates into window-local
//! paint coordinates. The view owns no animation of its own: the flyout
//! spring feeds the panel rect frame by frame, and the source anchor rides
//! the panel's center line (unclamped, so it never snaps ahead to the next
//! card). Both ends share one clock and one height, so the line stays a
//! straight, steady segment that arrives at both ends together.

use gpui::{
    canvas, div, point, px, Context, EventEmitter, IntoElement, ParentElement, PathBuilder, Pixels,
    Render, Styled, Window,
};

use crate::ui::design::palette::Palette;
use crate::ui::preview::connector::{
    panel_aligned_source, resolve_connector, ConnectorGeometry, ScreenPoint, ScreenRect,
};

/// Line stroke width in logical pixels (EcoPaste `strokeWidth="2"`).
const STROKE: f32 = 2.;
/// Endpoint dot radius (EcoPaste `r="4"`).
const DOT_RADIUS: f32 = 4.;
/// Endpoint dot core radius: reads as a ring on the panel edge.
const DOT_CORE: f32 = 2.2;

/// No-op event; the overlay is purely presentational.
pub enum PreviewConnectorEvent {}

impl EventEmitter<PreviewConnectorEvent> for PreviewConnectorView {}

pub struct PreviewConnectorView {
    /// Overlay window top-left in logical screen coordinates.
    origin: ScreenPoint,
    /// Hovered card rect; the source anchor slides along its side edge.
    card: Option<ScreenRect>,
    /// Current flyout rect in logical screen coordinates, fed frame by frame
    /// by the flyout spring.
    target_rect: Option<ScreenRect>,
}

impl PreviewConnectorView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            origin: (0., 0.),
            card: None,
            target_rect: None,
        }
    }

    /// Full retarget: the overlay window moved (`origin`) and/or the hovered
    /// card changed. While the flyout springs (`animate`), its frames feed the
    /// target rect — snapping to the goal here would draw one frame at the
    /// destination before the animation's first step arrives.
    pub fn retarget(
        &mut self,
        origin: ScreenPoint,
        card: Option<ScreenRect>,
        target_rect: ScreenRect,
        animate: bool,
        cx: &mut Context<Self>,
    ) {
        self.origin = origin;
        if let Some(card) = card {
            self.card = Some(card);
        }
        if !animate || self.target_rect.is_none() {
            self.target_rect = Some(target_rect);
        }
        cx.notify();
    }

    /// The flyout window rect changed (animation step or programmatic move).
    /// The line follows immediately so it stays glued to the panel edge.
    pub fn set_target_rect(&mut self, target_rect: ScreenRect, cx: &mut Context<Self>) {
        if self.target_rect == Some(target_rect) {
            return;
        }
        self.target_rect = Some(target_rect);
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.card = None;
        self.target_rect = None;
        cx.notify();
    }

    fn derived_geometry(&self) -> Option<ConnectorGeometry> {
        let card = self.card?;
        let target = self.target_rect?;
        Some(resolve_connector(
            panel_aligned_source(card, target),
            target,
        ))
    }
}

impl Render for PreviewConnectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(geometry) = self.derived_geometry() else {
            return div();
        };
        let origin = self.origin;
        let palette = Palette::current(cx);
        let line_color = palette.accent;
        let dot_core_color = palette.flyout;
        div().size_full().child(canvas(
            move |_, _, _| {},
            move |_, _, window, _| {
                let local = |p: ScreenPoint| point(px(p.0 - origin.0), px(p.1 - origin.1));
                // The curve itself: one stroked cubic Bezier.
                let mut line = PathBuilder::stroke(px(STROKE));
                line.move_to(local(geometry.source));
                line.cubic_bezier_to(
                    local(geometry.target),
                    local(geometry.control1),
                    local(geometry.control2),
                );
                // Endpoint dots: an accent ring with a panel-colored core.
                let mut dots = PathBuilder::fill();
                let mut cores = PathBuilder::fill();
                for center in [geometry.source, geometry.target] {
                    let center = local(center);
                    circle_subpath(&mut dots, center, DOT_RADIUS + STROKE / 2.);
                    circle_subpath(&mut cores, center, DOT_CORE);
                }
                if let Ok(line) = line.build() {
                    window.paint_path(line, line_color);
                }
                if let Ok(dots) = dots.build() {
                    window.paint_path(dots, line_color);
                }
                if let Ok(cores) = cores.build() {
                    window.paint_path(cores, dot_core_color);
                }
            },
        ))
    }
}

/// Append a filled circle subpath.
fn circle_subpath(path: &mut PathBuilder, center: gpui::Point<Pixels>, radius: f32) {
    let (cx, cy) = (f32::from(center.x), f32::from(center.y));
    let at = |angle: f32| point(px(cx + radius * angle.cos()), px(cy + radius * angle.sin()));
    path.move_to(at(0.));
    for step in 1..16 {
        path.line_to(at(step as f32 / 16. * std::f32::consts::TAU));
    }
    path.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_builder_builds_an_empty_path() {
        let path = PathBuilder::fill();
        assert!(path.build().is_ok());
    }

    #[test]
    fn circle_subpath_closes_a_full_ring() {
        let mut path = PathBuilder::fill();
        circle_subpath(&mut path, point(px(5.), px(5.)), 4.);
        assert!(path.build().is_ok());
    }

    #[test]
    fn stroked_bezier_builds() {
        let geometry = resolve_connector((0., 0., 40., 10.), (100., 0., 40., 10.));
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(point(px(geometry.source.0), px(geometry.source.1)));
        path.cubic_bezier_to(
            point(px(geometry.target.0), px(geometry.target.1)),
            point(px(geometry.control1.0), px(geometry.control1.1)),
            point(px(geometry.control2.0), px(geometry.control2.1)),
        );
        assert!(path.build().is_ok());
    }
}
