use gpui::{
    canvas, point, px, DispatchPhase, HitboxBehavior, IntoElement, ScrollDelta, ScrollHandle,
    Styled, TouchPhase, Window,
};
use std::{cell::RefCell, rc::Rc, time::Instant};

const PIXELS_PER_LINE: f32 = 100. / 3.;

#[derive(Clone)]
pub struct SmoothScroll {
    handle: ScrollHandle,
    glide: Rc<RefCell<WheelGlide>>,
}

impl SmoothScroll {
    pub fn new(handle: &ScrollHandle) -> Self {
        Self {
            handle: handle.clone(),
            glide: Rc::new(RefCell::new(WheelGlide::default())),
        }
    }

    pub fn cancel(&self) {
        self.glide.borrow_mut().cancel();
    }

    pub fn overlay(&self) -> impl IntoElement {
        let prepaint = self.clone();
        let scroll = self.clone();
        canvas(
            move |bounds, window, cx| {
                prepaint.advance(window, cx.reduce_motion());
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            },
            move |_, hitbox, window, _cx| {
                let mouse_scroll = scroll.clone();
                let mouse_hitbox = hitbox.clone();
                window.on_mouse_event(move |_: &gpui::MouseDownEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Capture && mouse_hitbox.is_hovered(window) {
                        mouse_scroll.cancel();
                    }
                });
                let scroll = scroll.clone();
                let view = window.current_view();
                window.on_mouse_event(move |event: &gpui::ScrollEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture
                        || event.touch_phase != TouchPhase::Moved
                        || !hitbox.should_handle_scroll(window)
                    {
                        return;
                    }
                    match event.delta {
                        ScrollDelta::Lines(lines) if lines.y != 0. => {
                            if scroll.scroll_by_lines(lines.y, window, cx.reduce_motion()) {
                                cx.notify(view);
                                cx.stop_propagation();
                            }
                        }
                        ScrollDelta::Pixels(_) => scroll.cancel(),
                        _ => {}
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    fn scroll_by_lines(&self, lines: f32, window: &Window, reduce_motion: bool) -> bool {
        let current_point = self.handle.offset();
        let current = f32::from(current_point.y);
        let max = f32::from(self.handle.max_offset().y);
        let mut glide = self.glide.borrow_mut();
        if glide.running() && glide.written != current {
            glide.cancel();
        }
        if reduce_motion {
            let running = glide.running();
            let target = glide.target(current).clamp(-max, 0.);
            let to = (target + lines * PIXELS_PER_LINE).clamp(-max, 0.);
            if (to - target).abs() < 1e-4 {
                return running;
            }
            glide.cancel();
            drop(glide);
            self.handle
                .set_offset(window.pixel_snap_point(point(current_point.x, px(to))));
            return true;
        }
        let moved = glide.scroll_by_lines(lines, current, max, Instant::now());
        moved || glide.running()
    }

    fn advance(&self, window: &mut Window, reduce_motion: bool) {
        if reduce_motion {
            self.cancel();
            return;
        }
        let current_point = self.handle.offset();
        let current = f32::from(current_point.y);
        let max = f32::from(self.handle.max_offset().y);
        let mut glide = self.glide.borrow_mut();
        let next = glide.advance(current, max, Instant::now());
        let running = glide.running();
        drop(glide);
        if let Some(next) = next {
            let offset = window.pixel_snap_point(point(current_point.x, px(next)));
            self.handle.set_offset(offset);
            self.glide.borrow_mut().written = f32::from(offset.y);
        }
        if running || next.is_some() {
            window.request_animation_frame();
        }
    }
}

#[derive(Default)]
struct WheelGlide {
    from: f32,
    to: f32,
    started: Option<Instant>,
    duration: Option<f32>,
    slope: f32,
    velocity: f32,
    written: f32,
}

fn segment(distance: f32) -> f32 {
    const RAMP_START: f32 = 120.;
    const RAMP_END: f32 = 480.;
    const MIN_FRAMES: f32 = 6.;
    const MAX_FRAMES: f32 = 12.;
    let slope = (MIN_FRAMES - MAX_FRAMES) / (RAMP_END - RAMP_START);
    let frames = (MAX_FRAMES + (distance.abs() - RAMP_START) * slope).clamp(MIN_FRAMES, MAX_FRAMES);
    frames / 60.
}

fn ease(slope: f32, x: f32) -> (f32, f32) {
    let (x1, y1, x2, y2) = (0.42f32, 0.42 * slope, 0.58f32, 1f32);
    let (cx, cy) = (3. * x1, 3. * y1);
    let (bx, by) = (3. * (x2 - x1) - cx, 3. * (y2 - y1) - cy);
    let (ax, ay) = (1. - cx - bx, 1. - cy - by);
    let curve_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let speed_x = |t: f32| (3. * ax * t + 2. * bx) * t + cx;
    let mut t = x;
    for _ in 0..8 {
        let error = curve_x(t) - x;
        let speed = speed_x(t);
        if error.abs() < 1e-6 || speed.abs() < 1e-6 {
            break;
        }
        t = (t - error / speed).clamp(0., 1.);
    }
    if (curve_x(t) - x).abs() > 1e-4 {
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..32 {
            t = (low + high) / 2.;
            if curve_x(t) < x {
                low = t
            } else {
                high = t
            }
        }
    }
    let y = ((ay * t + by) * t + cy) * t;
    let dy = (3. * ay * t + 2. * by) * t + cy;
    let dx = speed_x(t);
    (y, if dx.abs() < 1e-6 { 0. } else { dy / dx })
}

impl WheelGlide {
    fn progress(&self, now: Instant) -> f32 {
        match (self.started, self.duration) {
            (Some(started), Some(duration)) if duration > 0. => {
                (now.saturating_duration_since(started).as_secs_f32() / duration).min(1.)
            }
            _ => 0.,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let (progress, _) = ease(self.slope, self.progress(now));
        self.from + (self.to - self.from) * progress
    }

    fn velocity_at(&self, now: Instant) -> f32 {
        match self.duration {
            Some(duration) if duration > 0. => {
                let progress = self.progress(now);
                if progress >= 1. {
                    0.
                } else {
                    ease(self.slope, progress).1 * (self.to - self.from) / duration
                }
            }
            _ => self.velocity,
        }
    }

    fn running(&self) -> bool {
        self.started.is_some()
    }

    fn target(&self, current: f32) -> f32 {
        if self.running() && self.written == current {
            self.to
        } else {
            current
        }
    }

    fn scroll_by_lines(&mut self, lines: f32, current: f32, max: f32, now: Instant) -> bool {
        let target = self.target(current).clamp(-max, 0.);
        let to = (target + lines * PIXELS_PER_LINE).clamp(-max, 0.);
        if (to - target).abs() < 1e-4 {
            return false;
        }
        let (from, velocity) = if self.running() && self.written == current {
            (self.value(now), self.velocity_at(now))
        } else {
            (current, 0.)
        };
        *self = Self {
            from,
            to,
            started: Some(now),
            duration: None,
            slope: 0.,
            velocity,
            written: current,
        };
        true
    }

    fn advance(&mut self, current: f32, max: f32, now: Instant) -> Option<f32> {
        self.started?;
        if self.written != current {
            self.cancel();
            return None;
        }
        let to = self.to.clamp(-max, 0.);
        if self.duration.is_none() || to != self.to {
            if self.duration.is_some() {
                self.velocity = self.velocity_at(now);
                self.from = self.value(now);
                self.started = Some(now);
            }
            self.to = to;
            let distance = to - self.from;
            if distance.abs() < 0.01 {
                self.cancel();
                return Some(to);
            }
            let duration = segment(distance);
            self.duration = Some(duration);
            self.slope = (self.velocity * duration / distance).clamp(-1000., 1000.);
        }
        let mut offset = self.value(now).clamp(-max, 0.);
        self.written = offset;
        if self.progress(now) >= 1. {
            offset = to;
            self.written = offset;
            self.cancel();
        }
        Some(offset)
    }

    fn cancel(&mut self) {
        self.started = None;
        self.duration = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{ease, segment, WheelGlide, PIXELS_PER_LINE};
    use std::time::{Duration, Instant};

    #[test]
    fn durations_follow_distance() {
        assert!((segment(100.) - 0.2).abs() < 1e-6);
        assert!((segment(-120.) - 0.2).abs() < 1e-6);
        assert!((segment(300.) - 0.15).abs() < 1e-6);
        assert!((segment(900.) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn easing_preserves_retargeted_velocity() {
        let (half, slope) = ease(0., 0.5);
        assert!((half - 0.5).abs() < 1e-4);
        assert!(slope > 1.5);
        assert!(ease(0., 0.).1.abs() < 1e-4);
        assert!((ease(2., 0.).1 - 2.).abs() < 1e-3);
        assert!((ease(0., 1.).0 - 1.).abs() < 1e-5);
    }

    #[test]
    fn wheel_lines_glide_to_the_expected_target() {
        let started = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(-3., 0., 1_000., started));
        assert!((glide.target(0.) + 3. * PIXELS_PER_LINE).abs() < 1e-6);

        let midway = glide
            .advance(0., 1_000., started + Duration::from_millis(50))
            .expect("midway offset");
        assert!(midway < 0. && midway > -100.);
        let finished = glide
            .advance(midway, 1_000., started + Duration::from_millis(201))
            .expect("final offset");
        assert!((finished + 100.).abs() < 1e-6);
        assert!(!glide.running());
    }

    #[test]
    fn consecutive_notches_extend_the_current_target() {
        let started = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(-3., 0., 1_000., started));
        let midway = glide
            .advance(0., 1_000., started + Duration::from_millis(50))
            .expect("midway offset");
        assert!(glide.scroll_by_lines(-3., midway, 1_000., started + Duration::from_millis(60),));
        assert!((glide.target(midway) + 200.).abs() < 1e-6);
        let next = glide
            .advance(midway, 1_000., started + Duration::from_millis(60))
            .expect("retargeted offset");
        assert!(next <= midway);
    }

    #[test]
    fn scrolling_is_clamped_at_both_edges() {
        let now = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(!glide.scroll_by_lines(3., 0., 1_000., now));
        assert!(!glide.running());
        assert!(glide.scroll_by_lines(-30., 0., 1_000., now));
        assert!((glide.target(0.) + 1_000.).abs() < 1e-4);
        assert!(!glide.scroll_by_lines(-3., 0., 1_000., now));
        assert!(glide.running());
    }

    #[test]
    fn external_offset_changes_cancel_the_glide() {
        let started = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(-3., 0., 1_000., started));
        let midway = glide
            .advance(0., 1_000., started + Duration::from_millis(50))
            .expect("midway offset");
        assert_eq!(
            glide.advance(midway - 40., 1_000., started + Duration::from_millis(60)),
            None
        );
        assert!(!glide.running());
        assert_eq!(glide.target(midway - 40.), midway - 40.);
    }
}
