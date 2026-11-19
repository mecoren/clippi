//! 控件工厂：页面层唯一的原生控件来源。
//!
//! 对齐 Argus `frontend/widgets.rs` 的分工，但把与实体耦合的
//! `impl Desktop` 方法改成本仓的自由函数：首参永远是 [`Palette`]，
//! 页面自己 `let p = Palette::current(cx);` 后逐层组合。
//!
//! # 分层与外观约定
//!
//! 1. 颜色只从 [`Palette`] 取，尺寸只从 [`size`] 档位、字号只从
//!    [`typography`] 取；本文件不出现颜色字面量。
//! 2. **只有对话框带阴影**：卡片、菜单、提示都靠 1 px 描边 + `layer`
//!    半透明面做亮度差分层（Argus 的 Fluent 分层，为后续 Mica 预留）；
//!    投影仅 [`flyout_shadow`] 一处，对应 Argus `--shadow-flyout`。
//! 3. `on_card` 系列是"半透明面叠在卡片上"的不透明等效色；卡片内部的
//!    控件仍用 `control_face` 这类半透明令牌，与 gpui-kit 组件保持一致。
//! 4. 控件不持有主题、不缓存状态；需要动效时用 `design::motion`，
//!    不在此处自写缓动。
//! 5. 页面不直接拼 gpui-kit 原生控件；无状态的构造器写在本文件，需要自带
//!    交互状态的组件（如 `components::wait_switch`）放进 `components/`。
//! 6. 图标只经 [`icon`] / [`icon_hover`]，名称先在 `ui/assets.rs` 的
//!    `product_icons!` 表登记；SVG 不继承宿主文字色，颜色必须显式传入。

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::base::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};

use super::design::motion;
use super::design::palette::Palette;
use super::design::size::{Ctrl, IconSize, Radius, Space};
use super::design::tokens;
use super::design::typography::{self, type_role};

// ---------------------------------------------------------------------------
// 基础工具：描边、叠加、图标
// ---------------------------------------------------------------------------

/// 去掉 alpha：把半透明令牌用于必须不透底的场合（贴图、文字）。
pub fn solid(color: Hsla) -> Hsla {
    Hsla { a: 1., ..color }
}

/// 提亮 8%：对应 Argus 界面里 hover 的 `filter: brightness(1.08)`。
pub fn brighter(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l * 1.08).min(1.),
        ..color
    }
}

/// Fluent 控件底部那道更重的 1 px 边：叠在控件 1 px 描边之上，两端各缩
/// 3 px。宿主控件必须 `relative`。
pub fn control_edge(p: &Palette) -> Div {
    div()
        .absolute()
        .left(px(3.))
        .right(px(3.))
        .bottom(px(-1.))
        .h(px(1.))
        .bg(p.control_edge)
}

/// 输入框聚焦时的强调底线（2 px），叠在控件底边上。
pub fn accent_edge(color: Hsla, width: Pixels) -> Div {
    div()
        .absolute()
        .left(px(3.))
        .right(px(3.))
        .bottom_0()
        .h(width)
        .bg(color)
}

/// `:focus-visible` 的 2 px 外环（距控件边缘 3 px）。宿主必须 `relative`，
/// `border` 传控件自身描边宽度。
pub fn focus_ring(color: Hsla, border: Pixels) -> Div {
    let out = -(border + px(3.));
    div()
        .absolute()
        .top(out)
        .bottom(out)
        .left(out)
        .right(out)
        .border_2()
        .border_color(color)
        // 焦点环圆角 = 控件圆角 + 3（Argus `focus_ring`）。
        .rounded(border + px(3.))
}

/// 输入框占位符：绝对定位铺在输入层下方，字号不随输入框自身字体变化，
/// 光标仍可见。`left`/`right` 是输入框的水平内边距。
pub fn placeholder(text: SharedString, left: Pixels, right: Pixels, p: &Palette) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(left)
        .right(right)
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(typography::font(type_role::BASE))
        .text_color(p.ink_3)
        .child(text)
}

/// 单个产品图标：名称在 [`super::assets::product_icons`] 表里登记，
/// 未登记的名字显示圆圈。尺寸只用 12/16/20 三档（`IconSize`）。
pub fn icon(name: impl Into<SharedString>, size: Pixels, color: Hsla) -> Div {
    let path = super::assets::icon_path(&name.into());
    div()
        .flex_shrink_0()
        .size(size)
        .child(svg().path(path).size(size).text_color(color))
}

/// 交互图标：与 [`icon`] 同一外观，但按 `control` 的播放记录施加一次
/// 悬停动效（[`icon_probe`] 启动）。静止时不请求动画帧。
pub fn icon_moving(
    control: impl Into<SharedString>,
    name: impl Into<SharedString>,
    size: Pixels,
    color: Hsla,
) -> AnyElement {
    icon_moving_inner(control.into(), name.into(), size, color, None)
}

/// 同 [`icon_moving`]，并保留指针悬停换色（图标按钮的强调反馈）。
pub fn icon_moving_hover(
    control: impl Into<SharedString>,
    name: impl Into<SharedString>,
    size: Pixels,
    normal: Hsla,
    hover: Hsla,
) -> AnyElement {
    icon_moving_inner(control.into(), name.into(), size, normal, Some(hover))
}

fn icon_moving_inner(
    control: SharedString,
    name: SharedString,
    size: Pixels,
    color: Hsla,
    hover: Option<Hsla>,
) -> AnyElement {
    let (motion, plays, since) = super::icon_motion::prepare(&control, &name);
    let path = super::assets::icon_path(&name);
    let glyph = match hover {
        Some(hover) => svg()
            .path(path)
            .size(size)
            .text_color(color)
            .hover(move |style| style.text_color(hover)),
        None => svg().path(path).size(size).text_color(color),
    };
    let Some(since) = since else {
        return div()
            .flex_shrink_0()
            .size(size)
            .child(glyph)
            .into_any_element();
    };
    div()
        .flex_shrink_0()
        .size(size)
        .child(glyph.with_animation(
            // 每次播放换一个 id，保证播放期间持续出帧；姿态用播放自身的
            // 时钟采样，控件中途重绘也能接着走。
            ElementId::NamedInteger("icon-motion".into(), plays),
            Animation::new(motion.duration()),
            move |glyph, _| {
                let t = motion.played(Some(since), Instant::now()).unwrap_or(1.);
                let pose = motion.pose(t);
                if pose == super::icon_motion::Pose::REST {
                    return glyph;
                }
                let moved = glyph.with_transformation(
                    Transformation::scale(gpui::size(pose.scale.0, pose.scale.1))
                        .with_rotation(radians(pose.turn))
                        .with_translation(point(px(pose.shift.0), px(pose.shift.1))),
                );
                if pose.alpha < 1. {
                    moved.text_color(color.opacity(pose.alpha))
                } else {
                    moved
                }
            },
        ))
        .into_any_element()
}

/// 覆盖在控件上的透明悬停探针：指针进入时启动该控件的图标动效。
///
/// 宿主必须 `relative`。探针不 `occlude`：gpui 的命中测试会把指针下所有
/// 未阻断的 hitbox 都算作 hovered，因此父按钮的 hover 底色与图标自身的
/// 悬停换色都不受影响，点击也照常冒泡。`cursor` 与父控件一致，避免指针
/// 移到图标上时光标闪回箭头。
pub fn icon_probe(control: impl Into<SharedString>, cursor: CursorStyle) -> Stateful<Div> {
    let control: SharedString = control.into();
    let play = control.clone();
    div()
        .id(ElementId::Name(format!("icon-probe:{control}").into()))
        .absolute()
        .inset_0()
        .cursor(cursor)
        .hover(|style| style)
        .on_hover(move |hovered, _window, cx| {
            if *hovered && !cx.reduce_motion() {
                super::icon_motion::trigger(&play);
            }
        })
}

// ---------------------------------------------------------------------------
// 按钮
// ---------------------------------------------------------------------------

/// 按钮面色：普通、强调、危险、幽灵（无底无描边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Plain,
    Accent,
    Danger,
    Ghost,
}

impl Tone {
    /// (底, 字, 描边)
    fn face(self, p: &Palette) -> (Hsla, Hsla, Hsla) {
        let clear = p.background.opacity(0.);
        match self {
            Tone::Plain => (p.control_face, p.ink, p.stroke),
            Tone::Accent => (p.accent, p.on_accent, clear),
            Tone::Danger => (p.danger, p.on_danger, clear),
            Tone::Ghost => (clear, p.ink, clear),
        }
    }

    fn hover(self, p: &Palette) -> Hsla {
        match self {
            Tone::Plain => p.control_hover,
            Tone::Ghost => p.hover,
            Tone::Accent => brighter(p.accent),
            Tone::Danger => brighter(p.danger),
        }
    }

    /// (底, 字)
    fn press(self, p: &Palette) -> (Hsla, Hsla) {
        match self {
            Tone::Plain | Tone::Ghost => (p.press, p.ink_2),
            Tone::Accent => (p.accent.opacity(0.9), p.on_accent),
            Tone::Danger => (p.danger.opacity(0.9), p.on_danger),
        }
    }
}

/// 标准按钮（32 px）。返回值可继续链式 `.on_click(...)`。
pub fn btn(
    p: &Palette,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    glyph: Option<&str>,
    tone: Tone,
) -> Button {
    button_face(p, id.into(), text.into(), glyph, tone, false, None)
}

/// 小号按钮（24 px）。
pub fn btn_small(
    p: &Palette,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    glyph: Option<&str>,
    tone: Tone,
) -> Button {
    button_face(p, id.into(), text.into(), glyph, tone, true, None)
}

/// 小号按钮 + 自定义字号图标（`size` 为中心图标边长）：保留 8 px 内边距，
/// 不按方形图标钮收窄，用于"图标 + 文案"里需要更大字形的一行。
pub fn btn_small_glyph(
    p: &Palette,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    glyph: &str,
    size: f32,
    tone: Tone,
) -> Button {
    button_face(
        p,
        id.into(),
        text.into(),
        Some(glyph),
        tone,
        true,
        Some(px(size)),
    )
}

/// 卡片/工具条上的 24×24 幽灵图标钮：无底无描边，悬停出 `hover` 面并把
/// 图标提亮到强调色；禁用态压到 35% 且不响应。用于后端卡片这类「一行三个
/// 小动作」的位置，替代页面各自拼的 `icon_button` / `disabled_icon_button`。
#[allow(clippy::too_many_arguments)]
pub fn tool_icon_btn(
    p: &Palette,
    id: impl Into<ElementId>,
    glyph: &str,
    tint: Hsla,
    tooltip_text: &'static str,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let p = *p;
    let size = Ctrl::Small.scaled();
    let id: ElementId = id.into();
    let control = element_control(&id);
    let probe_control = control.clone();
    let on_click = std::rc::Rc::new(on_click);
    div()
        .id(id)
        .role(Role::Button)
        .tab_stop(!disabled)
        .aria_label(tooltip_text)
        .relative()
        .w(size)
        .h(size)
        .rounded(Radius::Md.scaled())
        .flex()
        .items_center()
        .justify_center()
        .cursor(if disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
        .when(!disabled, move |button| {
            button
                .hover(move |style| style.bg(p.hover))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_click(window, cx);
                })
                .child(icon_probe(probe_control, CursorStyle::PointingHand))
        })
        .when(disabled, |button| button.opacity(0.35))
        .tooltip(move |window, cx| tooltip(tooltip_text, window, cx))
        .child(icon_moving_hover(
            control,
            glyph,
            IconSize::Sm.scaled(),
            tint,
            p.accent,
        ))
        .into_any_element()
}

/// 只有图标的方形按钮（边长等于按钮高度）。
pub fn icon_btn(
    p: &Palette,
    id: impl Into<ElementId>,
    glyph: &str,
    size: Ctrl,
    tone: Tone,
) -> Button {
    let mut button = button_face(
        p,
        id.into(),
        SharedString::default(),
        Some(glyph),
        tone,
        size == Ctrl::Small,
        None,
    );
    button = button.h(size.px()).w(size.px()).justify_center();
    button
}

/// 禁用态的两个入口（Argus `ButtonExt`）：
///
/// - [`inert`] 只拦截输入，视觉不变（Fluent 的 `.btn:disabled` 本来就不变色）；
/// - [`inert_dimmed`] 额外压到半透明，用于下拉框与"录制中"的控件。
///
/// 页面不再各自拼 `.opacity(0.5)`。
pub trait ButtonExt {
    fn inert(self, disabled: bool) -> Self;
    fn inert_dimmed(self, disabled: bool) -> Self;
}

impl ButtonExt for Button {
    fn inert(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    fn inert_dimmed(self, disabled: bool) -> Self {
        self.disabled(disabled)
            .when(disabled, |button| button.opacity(0.5))
    }
}

/// 控件 id 对应的图标动效键。`ElementId` 没有反向字符串化，这里只覆盖
/// 本仓按钮实际使用的几种形态；其余共享一个兜底键。
fn element_control(id: &ElementId) -> SharedString {
    match id {
        ElementId::Name(name) => name.clone(),
        ElementId::NamedInteger(name, number) => format!("{name}:{number}").into(),
        ElementId::Integer(number) => format!("button:{number}").into(),
        // 其余形态（实体 id、uuid）在本仓按钮里没有使用；用 Debug 值兜底，
        // 至少不会让不同按钮共用同一个播放槽。
        other => format!("button:{other:?}").into(),
    }
}

fn button_face(
    p: &Palette,
    id: ElementId,
    text: SharedString,
    glyph: Option<&str>,
    tone: Tone,
    small: bool,
    glyph_size: Option<Pixels>,
) -> Button {
    let p = *p;
    let control = element_control(&id);
    let (bg, fg, border) = tone.face(&p);
    let height = if small {
        Ctrl::Small.scaled()
    } else {
        Ctrl::Btn.scaled()
    };
    let pad = if small {
        Space::Md.scaled()
    } else {
        Space::Lg.scaled()
    };
    let icon_only = text.is_empty() && glyph.is_some() && glyph_size.is_none();

    let mut button = Button::new(id)
        .relative()
        .h(height)
        .flex_shrink_0()
        .gap(Space::Sm.scaled())
        .rounded(Radius::Sm.scaled())
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(typography::font(type_role::BASE))
        .font_weight(typography::LABEL_WEIGHT)
        .whitespace_nowrap()
        .hover(move |style| style.bg(tone.hover(&p)))
        .active(move |style| {
            let (bg, fg) = tone.press(&p);
            style.bg(bg).text_color(fg)
        })
        .accessibility_label(text.clone())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

    button = if icon_only {
        button.w(height).justify_center()
    } else {
        button.px(pad)
    };
    if tone == Tone::Plain {
        button = button.child(control_edge(&p));
    }
    if let Some(glyph) = glyph {
        let size = glyph_size.unwrap_or_else(|| IconSize::Sm.scaled());
        button = button
            .child(icon_moving(control.clone(), glyph, size, fg))
            .child(icon_probe(control, CursorStyle::PointingHand));
    }
    if !text.is_empty() {
        button = button.child(text);
    }
    button
}

// ---------------------------------------------------------------------------
// Chip 与分段控件
// ---------------------------------------------------------------------------

/// 筛选条 chip（22 px，等宽、居中）。激活态用 15% 强调底。
pub fn chip(p: &Palette, id: impl Into<ElementId>, active: bool) -> Stateful<Div> {
    let p = *p;
    div()
        .id(id)
        .role(Role::Button)
        .tab_stop(true)
        .h(Ctrl::Chip.scaled())
        .flex_1()
        .min_w(px(0.))
        .flex_shrink_0()
        .rounded(Radius::Md.scaled())
        .bg(if active { p.accent_soft() } else { p.hover })
        .text_color(if active { p.ink } else { p.ink_2 })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| {
            if active {
                style
            } else {
                style.bg(p.hover_strong)
            }
        })
}

/// 第 `position` 格在 `count` 等分轨道里的 (左偏移, 宽度)，均为父宽分数。
pub(crate) fn segment_slot(position: f32, count: usize) -> (f32, f32) {
    let count = count.max(1) as f32;
    let index = position.clamp(0., count - 1.);
    (index / count, 1. / count)
}

// ---------------------------------------------------------------------------
// 输入框
// ---------------------------------------------------------------------------

/// 单行输入框：32 px，`control` 面 + 1 px 描边 + 更重的底边；聚焦时底边
/// 换成 2 px 强调线。调用方拿 `InputState` 的实体，外观全在这里。
pub fn text_field(
    p: &Palette,
    state: &Entity<InputState>,
    placeholder_text: impl Into<SharedString>,
    enabled: bool,
    window: &Window,
    cx: &App,
) -> Div {
    let p = *p;
    let pad = Space::Md.scaled();
    let input_state = state.read(cx);
    let focused = enabled && input_state.focus_handle(cx).is_focused(window);
    let empty = input_state.value().is_empty();
    let placeholder_text: SharedString = placeholder_text.into();

    let face = div()
        .relative()
        .size_full()
        .px(pad)
        .flex()
        .items_center()
        .border_1()
        .border_color(p.stroke)
        .rounded(Radius::Sm.scaled())
        .bg(p.control_face)
        .when(!focused, |el| el.child(control_edge(&p)))
        .when(empty, |el| {
            el.child(placeholder(placeholder_text.clone(), pad, pad, &p))
        })
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .px_0()
                .py_0()
                .text_size(typography::font(type_role::BASE))
                .text_color(p.ink)
                .aria_label(placeholder_text),
        );

    div()
        .relative()
        .h(Ctrl::Btn.scaled())
        .child(face)
        .when(focused, |el| el.child(accent_edge(p.accent, px(2.))))
}

// ---------------------------------------------------------------------------
// 键帽、转圈
// ---------------------------------------------------------------------------

/// 键帽：一个带描边的按键，关闭状态用虚线。
pub fn keycaps(p: &Palette, keys: &[String], off: bool) -> Div {
    let p = *p;
    div()
        .flex()
        .items_center()
        .gap(Space::Xs.scaled())
        .children(keys.iter().map(move |key| {
            div()
                .px(Space::Md.scaled())
                .py(px(2.))
                .border_1()
                .border_b(px(2.))
                .when(off, |el| el.border_dashed())
                .border_color(if off { p.stroke } else { p.stroke_strong })
                .rounded(Radius::Sm.scaled())
                .bg(p.control_face)
                .text_size(typography::font(type_role::BODY))
                .text_color(if off { p.ink_3 } else { p.ink })
                .whitespace_nowrap()
                .child(key.clone())
        }))
}

/// 不定转圈：一圈淡轨道 + 四分之一圈强调弧，800 ms 一轮。
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> AnyElement {
    let track = color.opacity(0.22);
    div()
        .size(px(size))
        .flex_shrink_0()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(800)).repeat(),
            move |element, delta| {
                element.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let stroke = (size / 8.).max(1.5);
                            let center = bounds.center();
                            let radius = f32::from(bounds.size.width) / 2. - stroke / 2.;
                            let ring = |from: f32, sweep: f32| {
                                let mut path = PathBuilder::stroke(px(stroke));
                                let steps = 24;
                                for step in 0..=steps {
                                    let angle = from + sweep * step as f32 / steps as f32;
                                    let at = point(
                                        center.x + px(radius * angle.cos()),
                                        center.y + px(radius * angle.sin()),
                                    );
                                    if step == 0 {
                                        path.move_to(at);
                                    } else {
                                        path.line_to(at);
                                    }
                                }
                                path.build().ok()
                            };
                            let tau = std::f32::consts::TAU;
                            if let Some(path) = ring(0., tau) {
                                window.paint_path(path, track);
                            }
                            if let Some(path) = ring(delta * tau - tau / 4., tau / 4.) {
                                window.paint_path(path, color);
                            }
                        },
                    )
                    .size_full(),
                )
            },
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 状态胶囊与下拉框
// ---------------------------------------------------------------------------

/// 状态胶囊的语义色（Argus `.state-pill` 的 tone 字符串）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillTone {
    /// 正常/在线。
    Ok,
    /// 注意/异常。
    Warn,
    /// 进行中：圆点换成转圈。
    Busy,
    /// 未启用/未知。
    Idle,
}

/// 状态胶囊：26 px 高的胶囊，`layer-strong` 面 + 1 px 描边，前置一个语义色
/// 圆点；`Busy` 用 10 px 转圈代替圆点。同步、传输这类"一行状态"用它，页面
/// 不再各写散装色点。
pub fn state_pill(
    p: &Palette,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    tone: PillTone,
) -> AnyElement {
    let p = *p;
    let color = match tone {
        PillTone::Ok => p.ok,
        PillTone::Warn => p.warn,
        PillTone::Busy => p.accent,
        PillTone::Idle => p.ink_3,
    };
    let marker = if tone == PillTone::Busy {
        spinner(id, 10., p.accent)
    } else {
        div()
            .size(px(8.))
            .flex_shrink_0()
            .rounded_full()
            .bg(color)
            .into_any_element()
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(Space::Sm.scaled())
        .h(Ctrl::Chip.scaled() + px(4.))
        .px(Space::Lg.scaled())
        .border_1()
        .border_color(p.stroke)
        .rounded(Radius::Pill.scaled())
        .bg(p.layer_strong)
        .text_size(typography::font(type_role::BODY))
        .text_color(p.ink)
        .whitespace_nowrap()
        .child(marker)
        .child(text.into())
        .into_any_element()
}

/// 下拉框按钮（Argus `select`）：32 高、`control` 面 + 1 px 描边 + 加重底边，
/// 宽度按最长选项定（其余选项以 0 高占位参与排版），右侧 12 px chevron。
///
/// 浮层由调用方在同一 `relative` 容器里渲染 [`select_list`]：Clippi 的页面
/// 各自持有开合状态，工厂不接管状态机（Argus 把状态放在 `Desktop` 上）。
/// 键盘：按钮聚焦时 ↑/↓/Enter 展开，Esc 收起。
#[allow(clippy::too_many_arguments)]
pub fn select(
    p: &Palette,
    id: impl Into<SharedString>,
    name: impl Into<SharedString>,
    options: &[String],
    selected: Option<usize>,
    open: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    select_with(
        p,
        id.into(),
        name.into(),
        options,
        selected,
        open,
        disabled,
        false,
        on_toggle,
    )
}

/// 紧凑下拉（Argus `select.compact`）：18 高、无描边无底，只按当前选项定宽，
/// 悬停/展开时浮出 `control-hover` 面。用在设置行的副标题位置。
#[allow(clippy::too_many_arguments)]
pub fn select_compact(
    p: &Palette,
    id: impl Into<SharedString>,
    name: impl Into<SharedString>,
    options: &[String],
    selected: Option<usize>,
    open: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    select_with(
        p,
        id.into(),
        name.into(),
        options,
        selected,
        open,
        false,
        true,
        on_toggle,
    )
}

#[allow(clippy::too_many_arguments)]
fn select_with(
    p: &Palette,
    id: SharedString,
    name: SharedString,
    options: &[String],
    selected: Option<usize>,
    open: bool,
    disabled: bool,
    compact: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    let p = *p;
    let current = selected
        .and_then(|index| options.get(index))
        .cloned()
        .unwrap_or_default();
    let on_toggle = std::rc::Rc::new(on_toggle);
    let toggle = on_toggle.clone();

    let mut button = Button::new(ElementId::Name(id.clone()))
        .role(Role::ComboBox)
        .accessibility_label(name)
        .key_context("Combobox")
        .relative()
        .flex_shrink_0()
        .justify_start()
        .rounded(Radius::Sm.scaled());

    button = if compact {
        button
            .min_w_0()
            .h(px(18.))
            .gap(Space::Xxs.scaled())
            .px(px(4.))
            .border_1()
            .border_color(p.stroke.opacity(0.))
            .text_size(typography::font(type_role::CAPTION))
            .text_color(p.ink_2)
            .when(open, move |button| button.bg(p.control_hover))
            .hover(move |style| style.bg(p.control_hover))
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(current),
            )
    } else {
        button
            .h(Ctrl::Btn.scaled())
            .pl(Space::Lg.scaled())
            .pr(Space::Md.scaled())
            .gap(Space::Md.scaled())
            .border_1()
            .border_color(p.stroke)
            .bg(if open {
                p.control_hover
            } else {
                p.control_face
            })
            .text_size(typography::font(type_role::BASE))
            .text_color(p.ink)
            .hover(move |style| style.bg(p.control_hover))
            .active(move |style| style.text_color(p.ink_2))
            .child(control_edge(&p))
            // 所有选项排在同一列、非选中项 0 高：按钮宽度即最长选项的宽度；
            // 被外层 `max_w` 限宽时裁掉，而不是把按钮撑开。
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(options.iter().enumerate().map(|(index, option)| {
                        div()
                            .whitespace_nowrap()
                            .when(Some(index) != selected, |option| {
                                option.h_0().overflow_hidden().invisible()
                            })
                            .child(option.clone())
                    })),
            )
    };

    button
        .child(icon("ChevronDownIcon", IconSize::Sm.scaled(), p.ink_2))
        .inert_dimmed(disabled)
        .on_click(move |_, window, cx| toggle(window, cx))
        .on_key_down({
            let toggle = on_toggle.clone();
            move |event: &KeyDownEvent, window, cx| {
                if disabled {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" | "space" | "down" if !open => {
                        toggle(window, cx);
                        cx.stop_propagation();
                    }
                    "escape" if open => {
                        toggle(window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }
        })
}

/// 下拉浮层：`flyout` 不透明面、1 px `stroke-strong`、8 px 圆角、**无阴影**
/// （Argus 的菜单与列表都不投影）。行 32 高、选中项右侧打勾。
pub fn select_list(
    p: &Palette,
    id: impl Into<ElementId>,
    options: &[String],
    selected: Option<usize>,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let p = *p;
    let on_pick = std::rc::Rc::new(on_pick);
    let mut list = div()
        .id(id)
        .rounded(Radius::Lg.scaled())
        .border_1()
        .border_color(p.stroke_strong)
        .bg(solid(p.flyout))
        .shadow_none()
        .p(Space::Xs.scaled())
        .flex()
        .flex_col()
        .text_size(typography::font(type_role::BASE))
        .text_color(p.ink)
        .occlude();
    for (index, option) in options.iter().enumerate() {
        let active = Some(index) == selected;
        let on_pick = on_pick.clone();
        list = list.child(
            div()
                .id(("select-option", index))
                .role(Role::ListBoxOption)
                .tab_stop(true)
                .aria_label(option.clone())
                .h(Ctrl::Btn.scaled())
                .px(Space::Md.scaled())
                .flex()
                .items_center()
                .justify_between()
                .gap(Space::Md.scaled())
                .rounded(Radius::Sm.scaled())
                .whitespace_nowrap()
                .cursor_pointer()
                .when(active, |row| row.text_color(p.accent))
                .when(!active, move |row| {
                    row.hover(move |style| style.bg(p.hover))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(option.clone()),
                )
                .when(active, |row| {
                    row.child(icon("CheckIcon", IconSize::Sm.scaled(), p.accent))
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_pick(index, window, cx);
                }),
        );
    }
    list
}

/// 页面进场（Argus `enter / hold_entrance / page_entering`）：页面切换后
/// 内容在 250 ms 内沿方向滑入并淡入。
///
/// Argus 把整页搬到离屏图层搬运，只在每帧移动；本仓页面较小，这里直接给出
/// 进度与位移，由调用方作用在整块内容上（与设置页原有的 Tab 淡入同一做法）。
#[derive(Clone, Copy)]
pub struct PageEntrance {
    switched: Option<Instant>,
}

impl PageEntrance {
    /// 已落定的状态：`progress` 恒为 1。
    pub fn settled() -> Self {
        Self { switched: None }
    }

    /// 页面刚切换：下一帧从起点开始播。
    pub fn arm(&mut self) {
        self.switched = Some(Instant::now());
    }

    /// 首帧只量布局、没有呈现时重新计时（Argus `hold_entrance`）。
    pub fn hold(&mut self) {
        self.switched = Some(Instant::now());
    }

    /// 是否仍在进场（Argus `page_entering`）；`reduce_motion` 下恒为 false。
    pub fn page_entering(&self, cx: &App) -> bool {
        !cx.reduce_motion()
            && self
                .switched
                .is_some_and(|switched| switched.elapsed() < motion::slow())
    }

    /// 本帧进度 0→1；进场期间自行请求下一帧。
    pub fn progress(&mut self, window: &Window, cx: &App) -> f32 {
        let Some(switched) = self.switched else {
            return 1.;
        };
        if cx.reduce_motion() {
            self.switched = None;
            return 1.;
        }
        let t = (switched.elapsed().as_secs_f32() / motion::slow().as_secs_f32()).min(1.);
        if t >= 1. {
            self.switched = None;
            return 1.;
        }
        window.request_animation_frame();
        motion::ease_enter(t)
    }
}

/// 进场方向：决定滑入距离与轴向（Argus 32 px 横向、24 px 纵向）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntranceAxis {
    Horizontal,
    Vertical,
}

/// 包一层进场：按 [`PageEntrance::progress`] 施加不透明度与位移。
pub fn enter(
    entrance: &mut PageEntrance,
    content: impl IntoElement,
    axis: EntranceAxis,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let progress = entrance.progress(window, cx);
    let distance = if axis == EntranceAxis::Horizontal {
        32.
    } else {
        24.
    };
    let shift = (1. - progress) * distance;
    div()
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .opacity(progress)
        .when(progress < 1., |element| match axis {
            EntranceAxis::Horizontal => element.ml(px(shift)),
            EntranceAxis::Vertical => element.mt(px(shift)),
        })
        .child(content)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 容器：卡片、设置分组、设置行、工具栏、空状态
// ---------------------------------------------------------------------------

/// 通用卡片：`layer` 面 + 1 px 描边，无阴影。
pub fn card(p: &Palette) -> Div {
    div()
        .rounded(Radius::Card.scaled())
        .bg(p.layer)
        .border_1()
        .border_color(p.stroke)
        .flex()
        .flex_col()
        .overflow_hidden()
}

/// 设置分组：一枚卡片装下整组行，行间用内缩发丝线分隔；标题只在多行时
/// 出现（单行的行标题已经说明一切）。
pub fn settings_group(p: &Palette, title: &str, rows: Vec<AnyElement>) -> Div {
    settings_group_inner(p, title, rows, false)
}

/// 同 [`settings_group`]，但整组撑满父容器剩余的高度。版本页的发布说明
/// 是唯一可滚动区域，页面本身不滚动，所以只有这一个滚动条。
pub fn settings_group_fill(p: &Palette, title: &str, rows: Vec<AnyElement>) -> Div {
    settings_group_inner(p, title, rows, true)
}

fn settings_group_inner(p: &Palette, title: &str, rows: Vec<AnyElement>, stretch: bool) -> Div {
    let p = *p;
    let show_title = rows.len() > 1;

    let mut card = div()
        .rounded(Radius::Lg.scaled())
        .bg(p.layer)
        .border_1()
        .border_color(p.stroke)
        .flex()
        .flex_col()
        .overflow_hidden()
        .when(stretch, |card| card.flex_1().min_h(px(0.)));
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(div().h(px(1.)).mx(Space::Lg.scaled()).bg(p.stroke));
        }
        card = card.child(row);
    }

    div()
        .flex()
        .flex_col()
        .gap(Space::Sm.scaled())
        .when(stretch, |group| group.flex_1().min_h(px(0.)))
        .when(show_title, |group| {
            group.child(
                div()
                    .px(Space::Xs.scaled())
                    .text_size(typography::font(type_role::CAPTION))
                    .font_weight(typography::LABEL_WEIGHT)
                    .text_color(p.ink_3)
                    .child(title.to_owned()),
            )
        })
        .child(card)
}

/// 设置行的裸壳（无卡片外观，由分组提供）。控件自定义时直接用它，保证
/// 与标准行对齐。
pub fn row_shell() -> Div {
    div()
        .min_h(super::design::tokens::dimension("settingsRowHeight"))
        .px(Space::Lg.scaled())
        .py(Space::Lg.scaled())
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(Space::Md.scaled())
}

/// 设置行的左列：标题单行省略，描述可换行并把行撑高。
pub fn row_text(p: &Palette, label: &str, desc: &str) -> Div {
    div()
        .flex()
        .flex_1()
        .min_w(px(0.))
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .max_w_full()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(typography::font(type_role::BASE))
                .font_weight(typography::LABEL_WEIGHT)
                .text_color(p.ink)
                .child(label.to_owned()),
        )
        .child(
            div()
                .w_full()
                .text_size(typography::font(type_role::CAPTION))
                .line_height(tokens::line_height())
                .text_color(p.ink_3)
                .child(desc.to_owned()),
        )
}

/// 标准设置行：左标题/描述 + 右侧控件。
pub fn settings_row(p: &Palette, label: &str, desc: &str, control: impl IntoElement) -> Div {
    row_shell()
        .child(row_text(p, label, desc))
        .child(div().flex_shrink_0().child(control))
}

/// 工具栏条：22 px 高、等距、左右 8 px 内边距（筛选条与选择器共用）。
pub fn toolbar() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(Space::Sm.scaled())
        .h(Ctrl::Chip.scaled())
        .w_full()
        .px(Space::Md.scaled())
        .flex_shrink_0()
}

/// 空状态：字形 + 标题 + 说明，垂直居中。
pub fn empty_state(p: &Palette, glyph: &str, title: &str, detail: &str) -> Div {
    let p = *p;
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(Space::Sm.scaled())
        .py(Space::Xxl.scaled())
        .child(icon(glyph, IconSize::Lg.scaled() * 1.8, p.ink_3))
        .child(
            div()
                .text_size(typography::font(type_role::TITLE))
                .font_weight(typography::LABEL_WEIGHT)
                .text_color(p.ink_2)
                .child(title.to_owned()),
        )
        .when(!detail.is_empty(), |el| {
            el.child(
                div()
                    .max_w(px(320.))
                    .text_center()
                    .text_size(typography::font(type_role::BODY))
                    .text_color(p.ink_3)
                    .child(detail.to_owned()),
            )
        })
}

// ---------------------------------------------------------------------------
// 浮层外观：菜单、对话框、提示、滚动条
// ---------------------------------------------------------------------------

/// 对话框投影（对应 Argus `--shadow-flyout`）：两层同轴黑影，下层大而淡，
/// 上层小而实。Argus 只给对话框用它——菜单、列表、提示都没有阴影。
pub fn flyout_shadow(dark: bool) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., if dark { 0.42 } else { 0.14 }),
            offset: point(px(0.), px(if dark { 12. } else { 8. })),
            blur_radius: px(if dark { 32. } else { 28. }),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 0., if dark { 0.25 } else { 0.06 }),
            offset: point(px(0.), px(2.)),
            blur_radius: px(if dark { 8. } else { 6. }),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

/// 菜单/下拉面板：`flyout` 不透明面 + 1 px 描边 + 8 px 圆角，无阴影
/// （Argus 的菜单同样不带投影，层级靠面与描边）。
pub fn menu_panel(p: &Palette) -> Div {
    div()
        .rounded(Radius::Lg.scaled())
        .border_1()
        .border_color(p.stroke)
        .bg(solid(p.flyout))
        .shadow_none()
        .p(Space::Xs.scaled())
        .flex()
        .flex_col()
}

/// 菜单项：等宽图标列 + 文案，悬停出 `hover` 底。`danger` 用危险红。
pub fn menu_item(
    p: &Palette,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    glyph: Option<&str>,
    danger: bool,
) -> Stateful<Div> {
    let p = *p;
    let ink = if danger { p.danger } else { p.ink };
    let mut item = div()
        .id(id)
        .h(Ctrl::Small.scaled())
        .px(Space::Md.scaled())
        .flex()
        .items_center()
        .gap(Space::Md.scaled())
        .rounded(Radius::Sm.scaled())
        .text_size(typography::font(type_role::BODY))
        .text_color(ink)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(p.hover));
    if let Some(glyph) = glyph {
        item = item.child(icon(glyph, IconSize::Sm.scaled(), ink));
    }
    item.child(label.into())
}

/// 对话框面板：`flyout` 不透明面、1 px 描边、8 px 圆角，
/// 阴影取 [`flyout_shadow`]（两层同轴，全仓只有对话框带投影）。
pub fn dialog_panel(p: &Palette) -> Div {
    div()
        .rounded(Radius::Lg.scaled())
        .border_1()
        .border_color(p.stroke)
        .bg(solid(p.flyout))
        .shadow(flyout_shadow(p.dark))
        .flex()
        .flex_col()
        .overflow_hidden()
}

/// 对话框标题行。
pub fn dialog_title(p: &Palette, text: &str) -> Div {
    div()
        .px(Space::Xl.scaled())
        .pt(Space::Xl.scaled())
        .pb(Space::Sm.scaled())
        .text_size(typography::font(type_role::TITLE))
        .font_weight(typography::LABEL_WEIGHT)
        .text_color(p.ink)
        .child(text.to_owned())
}

/// 列表滚动条：统一宽度与"滚动时出现"策略。
pub fn scrollbar(handle: &ScrollHandle) -> Scrollbar {
    Scrollbar::vertical(handle).mode(ScrollbarMode::Scrolling)
}

/// 悬停提示的公共外观：系统 tooltip 字体、12 px、无阴影（阴影在 Mica 合成
/// 下会起光晕），带 CJK 回退链。
fn tooltip_face(
    mut tooltip: gpui_kit::component::tooltip::Tooltip,
    cx: &mut App,
) -> gpui_kit::component::tooltip::Tooltip {
    tooltip = tooltip
        .shadow_none()
        .font_family(typography::tooltip_family(cx))
        .text_size(typography::font(type_role::BODY))
        .line_height(relative(4. / 3.));
    tooltip.text_style().font_fallbacks = Some(typography::fallbacks(cx));
    tooltip
}

/// 单行悬停提示。全仓唯一的提示入口，页面不得直接构造
/// `gpui_kit::component::tooltip::Tooltip`。
pub fn tooltip(text: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> AnyView {
    let tooltip = gpui_kit::component::tooltip::Tooltip::new(text.into());
    tooltip_face(tooltip, cx).build(window, cx)
}

/// 多行悬停提示：每项一条、行距 4 px，用于帮助说明与成对描述。多行需要
/// 自定义内容，与本文件的 `wrapping_tooltip` / `tooltip_with_shortcut` 一样
/// 走 `Tooltip::element`；页面层不得直接调用。
pub fn tooltip_lines<I, S>(lines: I, window: &mut Window, cx: &mut App) -> AnyView
where
    I: IntoIterator<Item = S>,
    S: Into<SharedString>,
{
    let lines: Vec<SharedString> = lines.into_iter().map(Into::into).collect();
    let tooltip = gpui_kit::component::tooltip::Tooltip::element(move |_, _| {
        div()
            .flex()
            .flex_col()
            .gap(Space::Xs.scaled())
            .text_size(typography::font(type_role::BODY))
            .children(
                lines
                    .clone()
                    .into_iter()
                    .map(|line| div().whitespace_nowrap().child(line)),
            )
    });
    tooltip_face(tooltip, cx).build(window, cx)
}

/// 成句的悬停提示：自然宽度超过 `max` 时在 `max` 处换行，像系统 tooltip 的
/// `title` 一样；未超过时与短提示同样一行（Argus `wrapping_tooltip`）。
///
/// 提示层按 min-content 排版，文本的 min-content 就是不换行的宽度，所以这里
/// 先用文本系统量一次自然宽度，再决定是否给宽度上限。
pub fn wrapping_tooltip(
    text: impl Into<SharedString>,
    max: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let text: SharedString = text.into();
    let mut tooltip_font = gpui::font(typography::tooltip_family(cx));
    tooltip_font.fallbacks = Some(typography::fallbacks(cx));
    let size = typography::font(type_role::BODY);
    let natural = text
        .split('\n')
        .map(|line| {
            let run = TextRun {
                len: line.len(),
                font: tooltip_font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(SharedString::from(line.to_owned()), size, &[run], None)
                .width
        })
        .fold(px(0.), Pixels::max);
    let tooltip = gpui_kit::component::tooltip::Tooltip::element(move |_, _| {
        div()
            .when(natural > max, |text| text.w(max))
            .child(text.clone())
    });
    tooltip_face(tooltip, cx).build(window, cx)
}

/// 带快捷键的悬停提示（FR-COMP-05）：右侧补一排键帽，按键提示与按钮一一对应。
pub fn tooltip_with_shortcut(
    p: &Palette,
    text: impl Into<SharedString>,
    keys: &[String],
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let p = *p;
    let text: SharedString = text.into();
    let keys = keys.to_vec();
    let tooltip = gpui_kit::component::tooltip::Tooltip::element(move |_, _| {
        div()
            .flex()
            .items_center()
            .gap(Space::Md.scaled())
            .child(text.clone())
            .child(keycaps(&p, &keys, false))
    });
    tooltip_face(tooltip, cx).build(window, cx)
}

#[cfg(test)]
mod tests {
    // 显式导入：`use super::*` 会把 `gpui::*` 全量拉进测试模块，`#[test]`
    // 宏展开会撞上编译器递归上限。
    use super::{brighter, segment_slot, solid};
    use gpui::Hsla;

    #[test]
    fn solid_drops_alpha_without_touching_hue() {
        let dim = Hsla {
            a: 0.4,
            ..gpui::rgb(0x2e8f66).into()
        };
        let opaque = solid(dim);
        assert_eq!(opaque.a, 1.);
        assert_eq!((opaque.h, opaque.s), (dim.h, dim.s));
    }

    #[test]
    fn brighter_never_exceeds_full_lightness() {
        assert!(
            brighter(Hsla {
                l: 0.99,
                ..Hsla::black()
            })
            .l <= 1.
        );
        let mid = Hsla {
            l: 0.5,
            ..Hsla::black()
        };
        assert!((brighter(mid).l - 0.54).abs() < 1e-6);
    }

    #[test]
    fn segment_slots_split_the_track_evenly() {
        let (left, width) = segment_slot(0., 4);
        assert_eq!((left, width), (0., 0.25));
        let (left, width) = segment_slot(3., 4);
        assert!((left - 0.75).abs() < 1e-6);
        assert!((width - 0.25).abs() < 1e-6);
    }

    #[test]
    fn segment_slots_clamp_out_of_range_positions() {
        let (left, _) = segment_slot(9., 3);
        assert!((left - 2. / 3.).abs() < 1e-6);
        let (left, _) = segment_slot(-4., 3);
        assert_eq!(left, 0.);
        // 0 或空列表不会造成除零。
        assert_eq!(segment_slot(0., 0), (0., 1.));
    }
}
