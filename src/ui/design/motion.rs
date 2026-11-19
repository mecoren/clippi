//! 动效：全仓唯一的缓动曲线、时长与混色定义（Argus `theme.rs` 同款数值）。
//!
//! 组件动效一律走 `gpui_kit::base::motion::transition` + 本模块曲线；
//! 页面层不得自写 `|t| …` 闭包。

use std::time::Duration;

use gpui::{Hsla, Rgba};
use gpui_kit::base::motion::Transition;

use super::tokens::duration_ms;

/// 快速动效（167ms）：hover 变色、菜单淡入淡出、卡片工具条。
pub fn fast() -> Duration {
    Duration::from_millis(duration_ms("t-fast"))
}

/// 慢速动效（250ms）：分段滑块、页面/标签切换、对话框入场。
pub fn slow() -> Duration {
    Duration::from_millis(duration_ms("t-slow"))
}

/// CSS `cubic-bezier(x1,y1,x2,y2)` 在 `t` 处的取值（二分采样 x，曲线在
/// 本仓使用范围内单调）。移植自 Argus `theme.rs:525`。
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1. - s;
        3. * u * u * s * a + 3. * u * s * s * b + s * s * s
    };
    let (mut low, mut high) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (low + high) / 2.;
        if curve(x1, x2, mid) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    curve(y1, y2, (low + high) / 2.)
}

/// Fluent 的减速入场曲线（淡入、对话框入场）。
pub fn ease_out(t: f32) -> f32 {
    bezier(0.1, 0.9, 0.2, 1., t)
}

/// 位移曲线：滑块、抽屉、分段指示器滑移。
pub fn ease_move(t: f32) -> f32 {
    bezier(0.33, 0., 0.1, 1., t)
}

/// 页面/标签滑入曲线（不淡入）。
pub fn ease_enter(t: f32) -> f32 {
    bezier(0.33, 1., 0.68, 1., t)
}

/// 对称二次缓动（ease-in-out quad）：拖拽换位与落位共用。
pub fn ease_in_out_quad(t: f32) -> f32 {
    if t < 0.5 {
        2. * t * t
    } else {
        1. - (-2. * t + 2.).powi(2) / 2.
    }
}

/// 三次减速（ease-out cubic）：快窗指示器滑移、短提示淡入。
pub fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

/// 强减速缓动（ease-out quint）：滚动滑行"到位即停"，无回弹。
pub fn ease_out_damped(t: f32) -> f32 {
    1. - (1. - t).powi(5)
}

/// 线性 RGB 插值（含 alpha）；用于滚动渐隐这类纯色过渡。
pub fn mix_rgba(from: Rgba, to: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0., 1.);
    Rgba {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

/// `from` 向 `to` 过渡 `t`（预乘 sRGB 插值，含 alpha），与 CSS
/// `color-mix(in srgb, …)` 一致。`Hsla::blend` 保留首色 alpha，
/// 无法把半透明描边混向强调色，故这里自己实现（Argus `theme.rs:546`）。
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(from), Rgba::from(to));
    let t = t.clamp(0., 1.);
    let alpha = a.a + (b.a - a.a) * t;
    if alpha <= 0. {
        return Hsla::transparent_black();
    }
    let channel = |x: f32, y: f32| (x * a.a + (y * b.a - x * a.a) * t) / alpha;
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
    .into()
}

/// 组件的缺省过渡：`fast` + `ease_move`，跟随系统"减少动态效果"。
pub fn transition(cx: &gpui::App) -> Transition {
    if cx.reduce_motion() {
        Transition::new(Duration::ZERO)
    } else {
        Transition::new(fast()).ease(ease_move)
    }
}

/// `slow` + `ease_move` 的变体（滑块、指示器）。
pub fn transition_slow(cx: &gpui::App) -> Transition {
    if cx.reduce_motion() {
        Transition::new(Duration::ZERO)
    } else {
        Transition::new(slow()).ease(ease_move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_hits_endpoints_and_stays_in_range() {
        for curve in [(0.1, 0.9, 0.2, 1.), (0.7, 0., 1., 0.5), (0.33, 0., 0.1, 1.)] {
            let at0 = bezier(curve.0, curve.1, curve.2, curve.3, 0.);
            let at1 = bezier(curve.0, curve.1, curve.2, curve.3, 1.);
            assert!(at0.abs() < 1e-3, "start {at0}");
            assert!((at1 - 1.).abs() < 1e-3, "end {at1}");
            assert!(bezier(curve.0, curve.1, curve.2, curve.3, 0.5).is_finite());
        }
    }

    #[test]
    fn mix_reaches_both_ends_and_keeps_alpha() {
        let from = Hsla::transparent_black();
        let to: Hsla = gpui::rgb(0x2e8f66).into();
        assert!(Rgba::from(mix(from, to, 0.)).a < 1e-3);
        assert!((Rgba::from(mix(from, to, 1.)).a - 1.).abs() < 1e-6);
        let half = Rgba::from(mix(from, to, 0.5));
        assert!(half.a > 0.4 && half.a < 0.6);
    }
}
