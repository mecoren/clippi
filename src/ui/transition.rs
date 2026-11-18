//! `gpui_transitions` 的替代层（M0 依赖迁移，FR-MOTION-05）。
//!
//! `gpui_transitions` 0.1.5 已从依赖中删除，0.9 生态的对应物是
//! `gpui_kit::base::motion::transition`，但它与旧 `use_keyed_transition`
//! 有一处语义差异：新 API 首次出现某个 key 时直接吸附到目标值，而旧
//! API 由 `init` 闭包提供首帧初值、再向目标过渡（入场动画）。
//!
//! 本模块用「首帧零时长种子 + 帧内重定目标」复刻旧语义，保证迁移后
//! 视觉不变。M1 设计系统落地后动效统一收口到 `design/motion`，
//! 届时本模块随之退役。

use std::time::Duration;

use gpui::{App, ElementId, Window};
use gpui_kit::base::animation::Lerp;
use gpui_kit::base::motion::{self, Transition};

/// 旧式 `window.use_keyed_transition(key, cx, duration, |_, _| initial)`
/// + `.with_easing(easing)` + `update(target)` + `evaluate()` 的单调用等价。
///
/// - 首次出现某个 key：以零时长吸附到 `initial`，同一帧内重定目标为
///   `target`，随后按 `duration`/`easing` 从 `initial` 过渡到 `target`。
/// - 此后每帧向 `target` 插值；`target` 变化时从当前插值位置继续
///   （与旧 API 的 continuous 模式一致）。
/// - `easing` 传 `linear` 等价旧 API 不调用 `with_easing` 的默认。
pub fn keyed_transition<T>(
    window: &mut Window,
    cx: &mut App,
    key: impl Into<ElementId>,
    duration: Duration,
    initial: T,
    easing: impl Fn(f32) -> f32 + 'static,
    target: T,
) -> T
where
    T: Lerp + PartialEq + 'static,
{
    let id: ElementId = key.into();
    // AC-6：系统开启"减少动态效果"时不做补间，直接落定到目标值。
    let duration = if cx.reduce_motion() {
        Duration::ZERO
    } else {
        duration
    };
    // motion 的内部状态挂在 NamedChild(id, "__base-transition-state") 上，
    // 与这里同 id 的种子标记（布尔、不同条目）不会冲突。
    let seeded = window.use_keyed_state(id.clone(), cx, |_, _| false);
    if !*seeded.read(cx) {
        motion::transition(
            id.clone(),
            initial,
            Transition::new(Duration::ZERO),
            window,
            cx,
        );
        seeded.update(cx, |seeded, _| *seeded = true);
    }
    motion::transition(
        id,
        target,
        Transition::new(duration).ease(easing),
        window,
        cx,
    )
}
