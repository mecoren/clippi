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
//! 2. **卡片无阴影**：靠 1 px 描边 + `layer` 半透明面做亮度差分层
//!    （Argus 的 Fluent 分层，为后续 Mica 预留）。只有浮层——菜单、
//!    对话框、提示——带阴影。
//! 3. `on_card` 系列是"半透明面叠在卡片上"的不透明等效色；卡片内部的
//!    控件仍用 `control_face` 这类半透明令牌，与 gpui-kit 组件保持一致。
//! 4. 控件不持有主题、不缓存状态；需要动效时用 `design::motion`，
//!    不在此处自写缓动。
//! 5. 页面不直接拼 gpui-kit 原生控件；需要新外观时先在本文件加构造器。
//!
//! 过渡期（M2）本模块尚无调用方，`ui/mod.rs` 以 `allow(dead_code)`
//! 注册；M3 起逐屏替换，替换完一处就删一处依赖。

use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::base::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};

use super::design::motion;
use super::design::palette::Palette;
use super::design::size::{Ctrl, IconSize, Radius, Space};
use super::design::typography::{self, type_role};
use crate::core::i18n_keys::I18nKey;

/// 图标字体族：全仓既有约定（iconfont 打点字形），集中在此避免各处硬编码。
pub const ICON_FONT: &str = "iconfont";

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
        .rounded(px(7.))
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

/// 单个 iconfont 字形。
pub fn icon(glyph: impl Into<SharedString>, size: Pixels, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .font_family(ICON_FONT)
        .text_size(size)
        .text_color(color)
        .child(glyph.into())
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
    button_face(p, id.into(), text.into(), glyph, tone, false)
}

/// 小号按钮（24 px）。
pub fn btn_small(
    p: &Palette,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    glyph: Option<&str>,
    tone: Tone,
) -> Button {
    button_face(p, id.into(), text.into(), glyph, tone, true)
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
    );
    button = button.h(size.px()).w(size.px()).justify_center();
    button
}

fn button_face(
    p: &Palette,
    id: ElementId,
    text: SharedString,
    glyph: Option<&str>,
    tone: Tone,
    small: bool,
) -> Button {
    let p = *p;
    let (bg, fg, border) = tone.face(&p);
    let height = if small {
        Ctrl::Small.px()
    } else {
        Ctrl::Btn.px()
    };
    let pad = if small {
        Space::Md.px()
    } else {
        Space::Lg.px()
    };
    let icon_only = text.is_empty() && glyph.is_some();

    let mut button = Button::new(id)
        .relative()
        .h(height)
        .flex_shrink_0()
        .gap(Space::Sm.px())
        .rounded(Radius::Md.px())
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
        button = button.child(icon(glyph, IconSize::Sm.px(), fg));
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
        .h(Ctrl::Chip.px())
        .flex_1()
        .min_w(px(0.))
        .flex_shrink_0()
        .rounded(Radius::Md.px())
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

/// 分段控件：`layer` 底轨道 + 1 px 描边，抬起的选择块沿 `ease_move`
/// 滑到新位置。选项等宽（外层 `flex_1`），所以位置可以用父宽分数表示，
/// 不需要测量。
pub fn segments(
    p: &Palette,
    id: impl Into<SharedString>,
    items: Vec<(SharedString, bool)>,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let p = *p;
    let id: SharedString = id.into();
    let count = items.len().max(1);
    let selected = items.iter().position(|(_, on)| *on).unwrap_or(0);

    let position = gpui_kit::base::motion::transition(
        (ElementId::Name(id.clone()), "thumb"),
        selected as f32,
        motion::transition_slow(cx),
        window,
        cx,
    );
    let (left, width) = segment_slot(position, count);
    let on_pick = std::rc::Rc::new(on_pick);

    let thumb = div()
        .absolute()
        .top(px(2.))
        .bottom(px(2.))
        .left(px(2.))
        .right(px(2.))
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(left))
                .w(relative(width))
                .rounded(Radius::Sm.px())
                .border_1()
                .border_color(p.stroke)
                .bg(p.control_checked),
        );

    let mut track = div()
        .relative()
        .h(Ctrl::Chip.px())
        .rounded(Radius::Md.px())
        .border_1()
        .border_color(p.stroke)
        .bg(p.control_face)
        .flex()
        .items_center()
        .child(thumb);

    for (index, (label, on)) in items.into_iter().enumerate() {
        let on_pick = on_pick.clone();
        track = track.child(
            div()
                .id(ElementId::Name(format!("{id}:seg:{index}").into()))
                .relative()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(typography::font(type_role::BODY))
                .text_color(if on { p.ink } else { p.ink_2 })
                .when(on, |el| el.font_weight(typography::LABEL_WEIGHT))
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_pick(index, window, cx);
                }),
        );
    }

    track.into_any_element()
}

/// 第 `position` 格在 `count` 等分轨道里的 (左偏移, 宽度)，均为父宽分数。
fn segment_slot(position: f32, count: usize) -> (f32, f32) {
    let count = count.max(1) as f32;
    let index = position.clamp(0., count - 1.);
    (index / count, 1. / count)
}

// ---------------------------------------------------------------------------
// 开关
// ---------------------------------------------------------------------------

/// 开关：40×20 轨道 + 12 px 圆形滑块，滑块沿 `ease_move` 在 0/20 px 间
/// 滑动；关闭时只有描边，开启时填强调色。左侧带"开/关"字样，颜色之外
/// 多一层可读状态。
#[allow(clippy::too_many_arguments)]
pub fn toggle(
    p: &Palette,
    id: impl Into<SharedString>,
    name: impl Into<SharedString>,
    value: bool,
    disabled: bool,
    on_change: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let p = *p;
    let id: SharedString = id.into();
    let offset = gpui_kit::base::motion::transition(
        (ElementId::Name(id.clone()), "thumb"),
        if value { 20f32 } else { 0f32 },
        motion::transition(cx),
        window,
        cx,
    );
    let thumb = if value { p.on_accent } else { p.ink_3 };
    let track = if value {
        p.accent
    } else {
        p.background.opacity(0.)
    };
    let edge = if value { p.accent } else { p.ink_3 };

    div()
        .flex()
        .items_center()
        .gap(Space::Md.px())
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
                .w(px(40.))
                .h(px(20.))
                .flex_shrink_0()
                .rounded(px(10.))
                .border_1()
                .border_color(edge)
                .bg(track)
                .when(value, move |el| {
                    el.hover(move |style| style.bg(brighter(p.accent)))
                })
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(offset))
                        .w(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().size(px(12.)).rounded_full().bg(thumb)),
                )
                .disabled(disabled)
                .on_click(move |_, window, cx| on_change(window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
        )
        .into_any_element()
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
    let pad = Space::Md.px();
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
        .rounded(Radius::Md.px())
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
        .h(Ctrl::Btn.px())
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
        .gap(Space::Xs.px())
        .children(keys.iter().map(move |key| {
            div()
                .px(Space::Md.px())
                .py(px(2.))
                .border_1()
                .border_b(px(2.))
                .when(off, |el| el.border_dashed())
                .border_color(if off { p.stroke } else { p.stroke_strong })
                .rounded(Radius::Sm.px())
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
// 容器：卡片、设置分组、设置行、工具栏、空状态
// ---------------------------------------------------------------------------

/// 通用卡片：`layer` 面 + 1 px 描边，无阴影。
pub fn card(p: &Palette) -> Div {
    div()
        .rounded(Radius::Card.px())
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
    let p = *p;
    let show_title = rows.len() > 1;

    let mut card = div()
        .rounded(Radius::Lg.px())
        .bg(p.layer)
        .border_1()
        .border_color(p.stroke)
        .flex()
        .flex_col()
        .overflow_hidden();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(div().h(px(1.)).mx(Space::Lg.px()).bg(p.stroke));
        }
        card = card.child(row);
    }

    div()
        .flex()
        .flex_col()
        .gap(Space::Sm.px())
        .when(show_title, |group| {
            group.child(
                div()
                    .px(Space::Xs.px())
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
        .px(Space::Lg.px())
        .py(Space::Lg.px())
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(Space::Md.px())
}

/// 设置行的左列：标题单行省略，描述可换行并把行撑高。
pub fn row_text(p: &Palette, label: &str, desc: &str) -> Div {
    div()
        .flex()
        .flex_1()
        .min_w(px(0.))
        .flex_col()
        .gap(px(3.))
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
                .line_height(relative(1.4))
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
        .gap(Space::Sm.px())
        .h(Ctrl::Chip.px())
        .w_full()
        .px(Space::Md.px())
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
        .gap(Space::Sm.px())
        .py(Space::Xxl.px())
        .child(icon(glyph, IconSize::Lg.px() * 1.8, p.ink_3))
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

/// 菜单/下拉面板：`flyout` 不透明面 + 1 px 描边 + 8 px 圆角 + 一层投影。
pub fn menu_panel(p: &Palette) -> Div {
    div()
        .rounded(Radius::Lg.px())
        .border_1()
        .border_color(p.stroke)
        .bg(solid(p.flyout))
        .shadow_lg()
        .p(Space::Xs.px())
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
        .h(Ctrl::Small.px())
        .px(Space::Md.px())
        .flex()
        .items_center()
        .gap(Space::Md.px())
        .rounded(Radius::Sm.px())
        .text_size(typography::font(type_role::BODY))
        .text_color(ink)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(p.hover));
    if let Some(glyph) = glyph {
        item = item.child(icon(glyph, IconSize::Sm.px(), ink));
    }
    item.child(label.into())
}

/// 对话框面板：`flyout` 不透明面、1 px 描边、8 px 圆角、一层浮起投影。
pub fn dialog_panel(p: &Palette) -> Div {
    div()
        .rounded(Radius::Lg.px())
        .border_1()
        .border_color(p.stroke)
        .bg(solid(p.flyout))
        .shadow_lg()
        .flex()
        .flex_col()
        .overflow_hidden()
}

/// 对话框标题行。
pub fn dialog_title(p: &Palette, text: &str) -> Div {
    div()
        .px(Space::Xl.px())
        .pt(Space::Xl.px())
        .pb(Space::Sm.px())
        .text_size(typography::font(type_role::TITLE))
        .font_weight(typography::LABEL_WEIGHT)
        .text_color(p.ink)
        .child(text.to_owned())
}

/// 列表滚动条：统一宽度与"滚动时出现"策略。
pub fn scrollbar(handle: &ScrollHandle) -> Scrollbar {
    Scrollbar::vertical(handle).mode(ScrollbarMode::Scrolling)
}

/// 悬停提示：系统 tooltip 字体、12 px、无阴影（阴影在 Mica 合成下会起
/// 光晕），带 CJK 回退链。
pub fn tooltip(text: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> AnyView {
    let mut tooltip = gpui_kit::component::tooltip::Tooltip::new(text.into())
        .shadow_none()
        .font_family(typography::tooltip_family(cx))
        .text_size(typography::font(type_role::BODY))
        .line_height(relative(4. / 3.));
    tooltip.text_style().font_fallbacks = Some(typography::fallbacks(cx));
    tooltip.build(window, cx)
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
