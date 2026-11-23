use gpui::{
    canvas, point, px, DispatchPhase, HitboxBehavior, IntoElement, ScrollDelta, ScrollHandle,
    Styled, TouchPhase, Window,
};
use std::{cell::RefCell, rc::Rc, time::Instant};

const PIXELS_PER_LINE: f32 = 100. / 3.;

/// Sub-pixel slack for the "did someone else move the list" test.
///
/// The offset travels through two quantizations on its way back to us: this
/// module writes it device-pixel snapped, and gpui's scroll clamp rounds the
/// maximum offset to two decimals. At an edge both are active at once, so the
/// value read next frame differs from the one written by a fraction of a pixel
/// (`1128.5714` written, `1128.57` read). Comparing them for exact equality
/// cancelled the glide on its first frame, which is why a wheel notch at the
/// bottom moved the list by ~0.03px: many notches, almost no movement. A
/// tolerance keeps genuine external moves (scrollbar drag, programmatic jumps,
/// keyboard navigation — all far larger than half a pixel) detectable.
const OFFSET_EPSILON: f32 = 0.5;

/// Whether `current` was moved by something other than this glide.
fn moved_elsewhere(written: f32, current: f32) -> bool {
    (written - current).abs() > OFFSET_EPSILON
}

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
        let offset = self.handle.offset();
        let max = f32::from(self.handle.max_offset().y);
        let mut current = f32::from(offset.y).clamp(-max, 0.);
        // GPUI 自己的滚轮处理器只往 offset 上累加、不夹紧（夹紧发生在下一帧
        // prepaint），所以在边缘多滚一格就会把 offset 留在 `[-max, 0]` 之外。
        // 从这种位置起步：朝外的步进整格被夹掉（判成「没动」，事件又交回原生
        // 处理器继续往外累加），朝内的步进要等下一帧 prepaint 把 offset 夹回来
        // 才生效，观感就是滚到底以后「滚轮转不动」。这里先把 offset 拉回范围
        // 内再走一步，顺带让 `written` 与句柄里的值一致。
        let out_of_range = current != f32::from(offset.y);
        if out_of_range {
            self.glide.borrow_mut().cancel();
            let snapped = window.pixel_snap_point(point(offset.x, px(current)));
            self.handle.set_offset(snapped);
            // 用它真正落盘的值，后面的 `written` 才能和下一帧读到的 offset 相等。
            current = f32::from(snapped.y);
        }
        let mut glide = self.glide.borrow_mut();
        if glide.running() && moved_elsewhere(glide.written, current) {
            glide.cancel();
        }
        // 一整格都被边缘吞掉时也要吃掉事件：放给原生处理器它会继续往外累加，
        // 正好制造上面那个卡死的起点。
        let step_clipped =
            ((current + lines * PIXELS_PER_LINE).clamp(-max, 0.) - current).abs() < 1e-4;
        if reduce_motion {
            let running = glide.running();
            let target = glide.target(current).clamp(-max, 0.);
            let to = (target + lines * PIXELS_PER_LINE).clamp(-max, 0.);
            if (to - target).abs() < 1e-4 {
                return running || out_of_range || step_clipped;
            }
            glide.cancel();
            drop(glide);
            self.handle
                .set_offset(window.pixel_snap_point(point(offset.x, px(to))));
            return true;
        }
        let moved = glide.scroll_by_lines(lines, current, max, Instant::now());
        moved || glide.running() || out_of_range || step_clipped
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
        if self.running() && !moved_elsewhere(self.written, current) {
            self.to
        } else {
            current
        }
    }

    fn scroll_by_lines(&mut self, lines: f32, current: f32, max: f32, now: Instant) -> bool {
        // 起点可能落在 `[-max, 0]` 之外（见 `SmoothScroll::scroll_by_lines`），
        // 先夹到边上，否则 `to` 与 `target` 会被夹成同一个值而判成「没动」。
        let current = current.clamp(-max, 0.);
        let running = self.running() && !moved_elsewhere(self.written, current);
        let target = self.target(current).clamp(-max, 0.);
        let step = lines * PIXELS_PER_LINE;
        let to = (target + step).clamp(-max, 0.);
        if (to - target).abs() < 1e-4 {
            return false;
        }
        // 反向滚动必须当场响应：连滚几格后马上往回滚时，`target` 还停在反方向
        // 很远处，继续沿用目标只是把它往回拽，列表仍朝原方向冲，观感就是
        // 「往回滑但不动」。反向一律以当前可见位置为新起点并丢掉旧动量，
        // 一格就是一格。
        if running && (target - current) * step < 0. {
            *self = Self {
                from: current,
                to: (current + step).clamp(-max, 0.),
                started: Some(now),
                duration: None,
                slope: 0.,
                velocity: 0.,
                written: current,
            };
            return true;
        }
        let (from, velocity) = if running {
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
        if moved_elsewhere(self.written, current) {
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
    use super::{ease, segment, WheelGlide, OFFSET_EPSILON, PIXELS_PER_LINE};
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

    /// 溢出底边的起点以边缘为基准：朝内一格照旧能移动，朝外一格判成「没动」。
    ///
    /// GPUI 的原生滚轮处理器只累加 offset、不夹紧，边缘多滚一格就会把它留在
    /// `[-max, 0]` 之外，所以「起点越界」是常态而不是异常状态。
    #[test]
    fn a_step_from_past_the_bottom_still_moves_up() {
        let now = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(3., -1_040., 1_000., now));
        assert!((glide.target(-1_000.) + 900.).abs() < 1e-4);

        let mut at_the_bottom = WheelGlide::default();
        assert!(!at_the_bottom.scroll_by_lines(-3., -1_000., 1_000., now));
    }

    /// 反过来，溢出顶边的起点仍要能往下滚。
    #[test]
    fn a_step_from_past_the_top_still_moves_down() {
        let now = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(-3., 40., 1_000., now));
        assert!((glide.target(0.) + 100.).abs() < 1e-4);

        let mut at_the_top = WheelGlide::default();
        assert!(!at_the_top.scroll_by_lines(3., 0., 1_000., now));
    }

    /// 连滚几格后立刻往回滚，必须从当前可见位置当场反向。
    ///
    /// 旧实现沿用「已累积的目标」：上滚只是把停在下方很远的 `to` 往回拽，
    /// 列表继续朝原方向冲，用户看到的是「往回滑也不动」。
    #[test]
    fn reversing_mid_flight_starts_from_the_visible_position() {
        let started = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(-30., 0., 1_000., started));
        assert!((glide.target(0.) + 1_000.).abs() < 1e-4);

        let midway = glide
            .advance(0., 1_000., started + Duration::from_millis(40))
            .expect("midway offset");
        assert!(midway < -20.);

        assert!(glide.scroll_by_lines(3., midway, 1_000., started + Duration::from_millis(41)));
        let expected = midway + PIXELS_PER_LINE * 3.;
        assert!(
            (glide.target(midway) - expected).abs() < 1e-3,
            "target {} must sit one notch above the visible {}",
            glide.target(midway),
            midway
        );

        // 反向后的第一帧就该朝上走，而不是继续往下的动量。
        let next = glide
            .advance(midway, 1_000., started + Duration::from_millis(41))
            .expect("reversed offset");
        assert!(next >= midway);
    }

    /// 贴边时 offset 会被两套量化各裁一刀（本模块按设备像素取整，gpui 把
    /// `max_offset` 四舍五入到两位小数），回读值与写入值差千分之几像素。
    /// 这不算「外部改动了位置」：以前每一格只走一帧就被取消，于是滚到底后
    /// 要滑很多下才挪得动。
    #[test]
    fn sub_pixel_offset_drift_keeps_the_glide_running() {
        let max = 1128.5714_f32;
        let started = Instant::now();
        let mut glide = WheelGlide::default();
        assert!(glide.scroll_by_lines(3., -max, max, started));
        let first = glide
            .advance(-max, max, started + Duration::from_millis(1))
            .expect("first frame");
        assert!(first > -max);

        // 回读值被 gpui 的两位小数上限夹住
        let observed = -1128.57_f32;
        assert!((first - observed).abs() < OFFSET_EPSILON);

        let next = glide.advance(observed, max, started + Duration::from_millis(2));
        assert!(next.is_some(), "sub-pixel drift must not cancel the glide");
        assert!(glide.running());

        // 真正的外部改动（拖动滚动条、程序化跳转）仍要取消。
        assert_eq!(
            glide.advance(observed - 40., max, started + Duration::from_millis(3)),
            None
        );
        assert!(!glide.running());
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
