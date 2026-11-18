//! 开关（`WaitToggleSwitch`）：40×24 胶囊轨道 + 18 px 圆形滑块，左侧带
//! 「开/关」状态字，颜色之外多一层可读状态。
//!
//! 外观分三段亮度，而不是 Fluent 的「描边轨道 + 深色圆点」：
//!
//! - 开启：整条轨道填 `accent`，滑块取 `on_accent`，描边与底同色（视觉上没有
//!   边界）；
//! - 关闭：轨道是 `control` 面掺一档 `ink-3` 的抬起面 + 1 px 描边，滑块为不
//!   透明的 `control-checked` 再补一圈细描边。
//!
//! 「卡片 < 轨道 < 滑块」是组件唯一的分层手段：全仓约定只有对话框带投影
//! （见 `widgets::flyout_shadow`），控件不能靠阴影浮起来。
//!
//! 切换动画只有一个进度值（0=关、1=开）：滑块位移、轨道底色、滑块色与两处
//! 描边都由它派生，`t-slow` + `ease-move` 走完一段连续过渡，减弱动效时瞬时
//! 到位。后续开关类控件必须沿用这套动画，见 `AGENTS.md`「开关组件与动画」。

use std::rc::Rc;

use gpui::*;
use gpui_kit::base::Button;

use crate::core::i18n_keys::I18nKey;
use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::Space;
use crate::ui::design::typography::{self, type_role};
use crate::ui::widgets::{brighter, solid};

/// 轨道外框宽度。
const TRACK_W: f32 = 40.;
/// 轨道外框高度（胶囊直径）。
const TRACK_H: f32 = 24.;
/// 轨道描边宽度；开启态描边透明，但两态占位一致，滑动时滑块不会抖。
const BORDER: f32 = 1.;
/// 滑块与轨道内缘的间距。
const INSET: f32 = 2.;
/// 滑块直径。
const THUMB: f32 = 18.;
/// 滑块行程：轨道内宽减去两侧内边距与滑块自身。
const TRAVEL: f32 = TRACK_W - 2. * BORDER - 2. * INSET - THUMB;

/// 点击回调；调用方在自己的实体上翻转状态并重绘。
type ChangeHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// 开关。构建后直接作为 `child` 使用，渲染时自行读取 [`Palette`]。
#[derive(IntoElement)]
pub struct WaitToggleSwitch {
    id: SharedString,
    name: SharedString,
    value: bool,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl WaitToggleSwitch {
    /// `id` 同时是动画槽与元素 id，必须在同一视图内唯一；`name` 是无障碍名。
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            value: false,
            disabled: false,
            on_change: None,
        }
    }

    /// 当前状态：`true` 为开。
    pub fn value(mut self, value: bool) -> Self {
        self.value = value;
        self
    }

    /// 禁用只拦截点击，外观不变（与 Fluent 的 `.btn:disabled` 一致）。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 点击回调；调用方在自己的实体上翻转状态并重绘。
    pub fn on_change(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for WaitToggleSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            name,
            value,
            disabled,
            on_change,
        } = self;
        let p = Palette::current(cx);
        // 开关只有一个动画进度：0=关、1=开。位移与轨道/滑块/描边的颜色都由
        // 它派生，所以切换是一段连续过渡，而不是"滑块滑动 + 颜色跳变"。
        // 时长取 `t-slow`（250 ms）+ `ease-move`，减弱动效时退化为瞬时。
        let progress = gpui_kit::base::motion::transition(
            (ElementId::Name(id.clone()), "toggle-thumb"),
            if value { 1f32 } else { 0f32 },
            motion::transition_slow(cx),
            window,
            cx,
        )
        .clamp(0., 1.);

        // 关闭态轨道：`control` 面掺一档 `ink-3`，比卡片暗、比白色滑块更暗。
        let idle_track = motion::mix(p.control_face, p.ink_3, 0.14);
        let idle_hover = motion::mix(p.control_face, p.ink_3, 0.22);
        let track = motion::mix(idle_track, p.accent, progress);
        // 开启态描边与底同色（视觉上没有边界），关闭态才显出细边。
        let edge = motion::mix(p.stroke, p.accent.opacity(0.), progress);
        let thumb = motion::mix(solid(p.control_checked), p.on_accent, progress);
        // 滑块的细边同样淡出：白色滑块在亮轨道上要靠它定轮廓，落到强调色
        // 轨道上则不该留一圈灰边。
        let thumb_edge = motion::mix(p.stroke, p.stroke.opacity(0.), progress);
        let hover = motion::mix(idle_hover, brighter(p.accent), progress);

        div()
            .flex()
            .items_center()
            .gap(Space::Md.scaled())
            .flex_shrink_0()
            .child(
                div()
                    .min_w(px(20.))
                    .flex()
                    .justify_end()
                    .text_size(typography::font(type_role::BODY))
                    .text_color(p.ink_2)
                    .child(if value {
                        I18nKey::ToggleOn.text()
                    } else {
                        I18nKey::ToggleOff.text()
                    }),
            )
            .child(
                Button::new(ElementId::Name(id))
                    .role(Role::Switch)
                    .accessibility_label(name)
                    .relative()
                    .w(px(TRACK_W))
                    .h(px(TRACK_H))
                    .flex_shrink_0()
                    .rounded_full()
                    .border_1()
                    .border_color(edge)
                    .bg(track)
                    .hover(move |style| style.bg(hover))
                    .child(
                        div()
                            .absolute()
                            .top(px(INSET))
                            .left(px(INSET + TRAVEL * progress))
                            .size(px(THUMB))
                            .rounded_full()
                            .border_1()
                            .border_color(thumb_edge)
                            .bg(thumb),
                    )
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        if let Some(handler) = on_change.as_ref() {
                            handler(window, cx);
                        }
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
    }
}
