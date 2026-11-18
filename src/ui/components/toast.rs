//! Toast notification — brief overlay message that auto-dismisses.
//!
//! The toast is a lightweight, reusable component for showing transient
//! feedback messages (e.g. "No OCR text detected", "Copied to clipboard").
//! Auto-dismiss and the enter/exit animation live in the parent view; this
//! component only renders the visual element.
//!
//! Action buttons can be added to toasts for interactive notifications
//! (e.g. "Download" / "Later" for update notifications).
//!
//! 外观只取语义色板与尺寸档位（FR-COMP-12）：`tip` 面 + 1 px 描边、8 px 圆角、
//! **无投影**（Argus：toasts 不投影），进出动效由 `root.rs` 走 `motion`。

use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Ctrl, Radius, Space};
use crate::ui::design::typography::{self, type_role};
use std::rc::Rc;

use gpui::*;

type ToastClickHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// An action button shown on a toast.
pub struct ToastAction {
    pub label: String,
    pub on_click: ToastClickHandler,
    /// Primary buttons get filled accent background; secondary are outlined.
    pub primary: bool,
}

impl Clone for ToastAction {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            on_click: Rc::clone(&self.on_click),
            primary: self.primary,
        }
    }
}

/// Toast visual style — errors use danger (red), info uses accent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warn,
}

#[derive(IntoElement)]
pub struct Toast {
    message: String,
    palette: Option<Palette>,
    icon: Option<String>,
    actions: Vec<ToastAction>,
    kind: ToastKind,
}

impl Toast {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            palette: None,
            icon: None,
            actions: Vec::new(),
            kind: ToastKind::Info,
        }
    }

    /// 取色入口：页面在渲染时把当前色板交进来。
    pub fn palette(mut self, palette: Palette) -> Self {
        self.palette = Some(palette);
        self
    }

    pub fn actions(mut self, actions: Vec<ToastAction>) -> Self {
        self.actions = actions;
        self
    }

    pub fn kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Default auto-dismiss duration for toast notifications.
pub const TOAST_DURATION: std::time::Duration = std::time::Duration::from_secs(3);

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.palette.unwrap_or_else(|| Palette::current(cx));
        let text_color = match self.kind {
            ToastKind::Info => p.accent,
            ToastKind::Warn => p.danger,
        };
        let has_actions = !self.actions.is_empty();
        let mut row = div()
            .w_full()
            .px(Space::Xl.scaled())
            .py(Space::Md.scaled())
            .bg(solid_tip(&p))
            .rounded(Radius::Lg.scaled())
            .border_1()
            .border_color(p.stroke_strong)
            // toasts 不投影（Argus：menus, lists and toasts have none）。
            .shadow_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(Space::Md.scaled());

        // Centre the message when there are no action buttons; spread
        // left (message) / right (buttons) when actions are present.
        row = if has_actions {
            row.justify_between()
        } else {
            row.justify_center()
        };

        // Left: icon + message
        let mut left = div()
            .flex()
            .flex_row()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap(Space::Md.scaled());

        if let Some(icon) = self.icon {
            left = left.child(crate::ui::widgets::icon(
                icon,
                typography::font(type_role::BODY),
                text_color,
            ));
        }

        let mut message = div()
            .flex_1()
            .min_w(px(0.))
            .text_size(typography::font(type_role::BODY))
            .text_color(text_color)
            .child(self.message);
        if !has_actions {
            message = message.text_align(TextAlign::Center);
        }
        left = left.child(message);

        row = row.child(left);

        // Right: action buttons (matching ConfirmDialog button style)
        if !self.actions.is_empty() {
            let buttons = div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .gap(Space::Sm.scaled())
                .children(self.actions.into_iter().map(move |action| {
                    let is_primary = action.primary;
                    let btn_bg = if is_primary {
                        p.accent
                    } else {
                        p.background.opacity(0.)
                    };
                    let btn_text = if is_primary { p.on_accent } else { p.ink_2 };
                    div()
                        .h(Ctrl::Chip.scaled())
                        .px(Space::Sm.scaled())
                        .rounded(Radius::Sm.scaled())
                        .bg(btn_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| {
                            style.bg(if is_primary {
                                crate::ui::widgets::brighter(p.accent)
                            } else {
                                p.hover
                            })
                        })
                        .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                            (action.on_click)(window, cx);
                        })
                        .child(
                            div()
                                .text_size(typography::font(type_role::CAPTION))
                                .font_weight(typography::LABEL_WEIGHT)
                                .text_color(btn_text)
                                .child(action.label),
                        )
                }));
            row = row.child(buttons);
        }

        row
    }
}

/// `tip` 令牌在透明窗口上会透出下层，实色化后再画描边。
fn solid_tip(p: &Palette) -> Hsla {
    Hsla { a: 1., ..p.tip }
}
