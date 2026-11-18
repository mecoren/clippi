//! 分段 Tab（`WaitTabs`）：灰色胶囊轨道 + 白色抬起块，选中文字用强调色。
//!
//! 取 Orbit 移动端的 Tab 语言：轨道比面板略深一档，选中块是白色圆角面，
//! 未选中文字保持 `ink-2`，悬停时提亮到强调色。选中块位置由
//! `transition_slow` 在 250 ms 内滑到新格，跟随用户的减弱动效设置。
//!
//! 六个 Tab 等分可用宽度，位置用父宽分数表示，不需要测量。外观只用
//! `Palette` 与尺寸令牌，不投影（全仓约定只有对话框带投影）。

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Ctrl, Radius};
use crate::ui::design::typography::{self, type_role};
use crate::ui::widgets::segment_slot;

/// 点击回调：收到新选中的下标。
type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// 分段 Tab。构建后直接作为 `child` 使用，渲染时自行读取 [`Palette`]。
#[derive(IntoElement)]
pub struct WaitTabs {
    id: SharedString,
    labels: Vec<SharedString>,
    selected: usize,
    on_select: Option<SelectHandler>,
}

impl WaitTabs {
    /// `id` 同时是动画槽与元素 id，必须在同一视图内唯一。
    pub fn new(
        id: impl Into<SharedString>,
        labels: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            selected: 0,
            on_select: None,
        }
    }

    /// 当前下标；越界时退到第 0 个。
    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// 点击回调；调用方在自己的实体上切换页面并重绘。
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for WaitTabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            labels,
            selected,
            on_select,
        } = self;
        let p = Palette::current(cx);
        let count = labels.len().max(1);
        let selected = selected.min(count - 1);

        let position = gpui_kit::base::motion::transition(
            (ElementId::Name(id.clone()), "tabs-thumb"),
            selected as f32,
            motion::transition_slow(cx),
            window,
            cx,
        );
        let (left, width) = segment_slot(position, count);

        // 白色抬起块：轨道内缩 2 px，位置按等分分数计算。
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
                    .rounded(Radius::Md.scaled())
                    .bg(p.control_checked),
            );

        let mut track = div()
            .relative()
            .h(Ctrl::Btn.scaled())
            .rounded(Radius::Lg.scaled())
            .bg(p.background.blend(p.hover))
            .flex()
            .items_center()
            .child(thumb);

        for (index, label) in labels.into_iter().enumerate() {
            let on = index == selected;
            let on_select = on_select.clone();
            track = track.child(
                div()
                    .id(ElementId::Name(format!("{id}:tab:{index}").into()))
                    .role(Role::Tab)
                    .tab_stop(true)
                    .aria_label(label.clone())
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
                    .text_color(if on { p.accent } else { p.ink_2 })
                    .when(on, |el| el.font_weight(typography::LABEL_WEIGHT))
                    // 桌面端补一个 hover：只提亮文字，不在轨道上再铺色块。
                    .when(!on, |el| el.hover(move |style| style.text_color(p.accent)))
                    .child(label)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(handler) = on_select.as_ref() {
                            handler(index, window, cx);
                        }
                    }),
            );
        }

        track
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::widgets::segment_slot;

    #[test]
    fn six_tabs_claim_equal_slots() {
        // 设置页 6 个 Tab：第 3 个的抬起块落在 1/3 处，宽 1/6。
        let (left, width) = segment_slot(2., 6);
        assert!((left - 1. / 3.).abs() < 1e-6);
        assert!((width - 1. / 6.).abs() < 1e-6);
    }
}
