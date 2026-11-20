//! Transparent click-through overlay that paints the card → flyout connector.
//!
//! The overlay is a separate native window owned by the flyout (see
//! `platform::preview_overlay`) covering the region between the hovered card
//! and the preview panel. It only paints the EcoPaste-style bezier line and
//! its two endpoint dots; input falls through to the windows underneath.
//!
//! Coordinates: everything the view receives is logical **screen** pixels;
//! `origin` (the overlay window's top-left) translates into window-local
//! paint coordinates. Retargets animate over `design::motion`'s fast easing;
//! the flyout-window rect animation feeds `set_target_rect` frame by frame.

use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, px, Context, EventEmitter, IntoElement, ParentElement, PathBuilder, Pixels,
    Render, Styled, Task, Window,
};

use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::preview::connector::{
    mix_geometry, resolve_connector, ConnectorGeometry, ScreenPoint, ScreenRect,
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
    /// Hovered card rect in logical screen coordinates.
    source_rect: Option<ScreenRect>,
    /// Current flyout rect in logical screen coordinates.
    target_rect: Option<ScreenRect>,
    /// Geometry currently displayed (`None` while hidden).
    displayed: Option<ConnectorGeometry>,
    /// Retarget animation: geometry at start + start time. The target is
    /// always re-derived from the current state, so mid-flight target updates
    /// (the flyout window animating) are chased instead of restarting.
    anim: Option<(ConnectorGeometry, Instant)>,
    _anim_task: Option<Task<()>>,
}

impl PreviewConnectorView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            origin: (0., 0.),
            source_rect: None,
            target_rect: None,
            displayed: None,
            anim: None,
            _anim_task: None,
        }
    }

    pub fn source_rect(&self) -> Option<ScreenRect> {
        self.source_rect
    }

    /// Full retarget: the overlay window moved (`origin`) and/or the source
    /// card / target panel changed. `animate` interpolates from the currently
    /// displayed line; the first show snaps.
    pub fn retarget(
        &mut self,
        origin: ScreenPoint,
        source_rect: Option<ScreenRect>,
        target_rect: ScreenRect,
        animate: bool,
        cx: &mut Context<Self>,
    ) {
        self.origin = origin;
        if let Some(rect) = source_rect {
            self.source_rect = Some(rect);
        }
        self.target_rect = Some(target_rect);
        self.apply(animate, cx);
    }

    /// The flyout window rect changed (animation step or programmatic move).
    /// The line follows immediately so it stays glued to the panel edge.
    pub fn set_target_rect(&mut self, target_rect: ScreenRect, cx: &mut Context<Self>) {
        if self.target_rect == Some(target_rect) {
            return;
        }
        self.target_rect = Some(target_rect);
        self.apply(false, cx);
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.displayed = None;
        self.anim = None;
        self._anim_task = None;
        self.source_rect = None;
        self.target_rect = None;
        cx.notify();
    }

    fn apply(&mut self, animate: bool, cx: &mut Context<Self>) {
        let derived = self.derived_geometry();
        let Some(derived) = derived else {
            self.displayed = None;
            self.anim = None;
            self._anim_task = None;
            cx.notify();
            return;
        };
        let animate = animate && self.displayed.is_some() && !cx.reduce_motion();
        if animate {
            self.anim = Some((self.displayed.unwrap_or(derived), Instant::now()));
            self.start_anim_task(cx);
        } else {
            self.anim = None;
        }
        self.displayed = Some(derived);
        cx.notify();
    }

    fn start_anim_task(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(15))
                .await;
            let keep = this.update(cx, |view, cx| {
                let Some((_, start)) = view.anim else {
                    return false;
                };
                if start.elapsed() >= motion::fast() {
                    view.anim = None;
                }
                cx.notify();
                view.anim.is_some()
            });
            if !matches!(keep, Ok(true)) {
                break;
            }
        });
        self._anim_task = Some(task);
    }

    fn derived_geometry(&self) -> Option<ConnectorGeometry> {
        let source = self.source_rect?;
        let target = self.target_rect?;
        Some(resolve_connector(source, target))
    }

    /// Geometry for the current frame: the derived target mixed from the
    /// animation's starting line while the retarget animation runs.
    fn display_geometry(&self) -> Option<ConnectorGeometry> {
        let derived = self.derived_geometry()?;
        let (from, start) = self.anim?;
        let total = motion::fast().as_millis().max(1) as f32;
        let t = (start.elapsed().as_millis() as f32 / total).min(1.);
        Some(mix_geometry(&from, &derived, motion::ease_move(t)))
    }
}

impl Render for PreviewConnectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(geometry) = self.display_geometry().or(self.derived_geometry()) else {
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
