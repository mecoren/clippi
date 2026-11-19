//! Confirm dialog — modal overlay for destructive action confirmation.
//!
//! Reusable confirmation dialog with full-window backdrop, centered card,
//! and cancel/confirm buttons. Supports danger styling for destructive actions.
//! Used for delete confirmations (single/batch) and hotkey blacklist removal.
//!
//! --- # Usage ---
//!
//! --- ```ignore ---
//! --- ConfirmDialog::delete_single() ---
//! --- .on_confirm(|_window, cx| { /* delete logic */ }) ---
//! --- .on_cancel(|_window, cx| { /* dismiss */ }) ---
//! --- .render_animated(window, cx, generation) // generation: u64, bump on each show ---
//! --- ``` ---
//!
//! --- # 危险操作冷静期（时停） ---
//!
//! --- `.hold_seconds(n)`：确认钮在 n 秒倒计时走完前禁用，文案换成 ---
//! --- 「请阅读后果（Ns）」，Enter 也不放行（对齐 orbit DangerousConfirmDialog） ---
//! --- 的强制阅读口径）。倒计时本身由调用方维护并每秒重渲染。 ---

use crate::ui::font::fs;
use std::rc::Rc;
use std::time::Duration;

use crate::core::i18n_keys::I18nKey;
use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::Radius;
use crate::ui::transition::keyed_transition;
use crate::ui::widgets;
use crate::ui::widgets::ButtonExt;

type DialogHandler = Rc<dyn Fn(&mut Window, &mut App)>;

struct DialogOption {
    label: String,
    selected: bool,
    on_toggle: DialogHandler,
}

/// 对话框入场：慢速档 `t-slow`（250 ms，`ease_out`）。
fn dialog_anim_duration() -> Duration {
    motion::slow()
}

// --- Component ---

// --- 危险数据操作两步确认（AGENTS 6.9）---

/// 两步危险确认的步骤：第 1 步确认影响范围 / 做出选择，第 2 步复述后果后
/// 最终执行。每步各有一次冷静期，第 1 → 第 2 步切换时重新起算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmStep {
    /// 第 1 步：确认影响范围 / 做出选择。
    Scope,
    /// 第 2 步：最终确认。
    Final,
}

/// 重要数据操作两步确认的冷静期（秒）：每步各一次，期间确认钮禁用。
pub const DANGEROUS_HOLD_SECONDS: u32 = 5;

/// 冷静期倒计时状态：挂在使用方自己的字段里，由 [`start_hold_countdown`] 驱动。
///
/// `epoch` 让旧倒计时任务在弹窗关闭重开、或步骤切换重新起算时自动退出，
/// 避免旧任务把新一轮冷静期多减一秒。
#[derive(Clone, Copy)]
pub struct HoldCountdown {
    epoch: u64,
    /// 剩余秒数：>0 时确认钮禁用并显示倒计时文案。
    remaining: u32,
}

impl HoldCountdown {
    /// 剩余秒数；0 表示冷静期已走完，确认钮可用。
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
}

/// 启动/重启一段冷静期倒计时：重置 `hold` 字段并每秒递减，归零停表。
/// 字段被清空（弹窗关闭）或被新一轮倒计时取代（epoch 不匹配）时自动结束，
/// 任务自行退出，调用方不需要保存或取消它。
pub fn start_hold_countdown<T: 'static>(
    this: &mut T,
    cx: &mut Context<T>,
    hold: impl Fn(&mut T) -> &mut Option<HoldCountdown> + Copy + 'static,
) {
    let epoch = {
        let slot = hold(this);
        let count = slot.get_or_insert_with(|| HoldCountdown {
            epoch: 0,
            remaining: 0,
        });
        count.epoch += 1;
        count.remaining = DANGEROUS_HOLD_SECONDS;
        count.epoch
    };
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let Ok(done) = this.update(cx, |this, cx| match hold(this) {
                Some(h) if h.epoch == epoch => {
                    if h.remaining > 0 {
                        h.remaining -= 1;
                        cx.notify();
                    }
                    h.remaining == 0
                }
                // 弹窗已关闭或已被新一轮倒计时取代：本轮作废。
                _ => true,
            }) else {
                return;
            };
            if done {
                return;
            }
        }
    })
    .detach();
}

pub struct ConfirmDialog {
    title: String,
    message: String,
    confirm_label: String,
    cancel_label: String,
    danger: bool,
    /// 危险操作冷静期（秒）：>0 时确认钮禁用并显示倒计时文案。
    hold_seconds: u32,
    on_confirm: Option<DialogHandler>,
    on_cancel: Option<DialogHandler>,
    options: Vec<DialogOption>,
    focus_handle: Option<FocusHandle>,
}

/// 构建器条件的 fluent 入口（`when` / `map`）：默认实现仅要求 `Sized`，
/// 不依赖 `IntoElement`，供调用方按状态条件追加 option 等配置。
impl FluentBuilder for ConfirmDialog {}

impl ConfirmDialog {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            confirm_label: I18nKey::DialogConfirm.text().into(),
            cancel_label: I18nKey::BtnCancel.text().into(),
            danger: false,
            hold_seconds: 0,
            on_confirm: None,
            on_cancel: None,
            options: Vec::new(),
            focus_handle: None,
        }
    }

    // --- Builder methods ---

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn confirm_label(mut self, label: impl Into<String>) -> Self {
        self.confirm_label = label.into();
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// 危险操作冷静期（时停）：倒计时走完前确认钮禁用、显示剩余秒数，
    /// Enter 同样不放行。秒数由调用方维护（每秒重渲染即可刷新文案）。
    pub fn hold_seconds(mut self, seconds: u32) -> Self {
        self.hold_seconds = seconds;
        self
    }

    pub fn on_confirm(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    pub fn on_cancel(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }

    /// Add a boolean option between the message and actions. May be called
    /// more than once to render multiple checkbox rows.
    pub fn option(
        mut self,
        label: impl Into<String>,
        selected: bool,
        on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.options.push(DialogOption {
            label: label.into(),
            selected,
            on_toggle: Rc::new(on_toggle),
        });
        self
    }

    /// Set a focus handle so keyboard events (Enter/Esc) reach the dialog.
    pub fn focus_handle(mut self, handle: FocusHandle) -> Self {
        self.focus_handle = Some(handle);
        self
    }

    // --- Preset factory methods ---

    /// Single item delete confirmation — simple yes/no.
    pub fn delete_single() -> Self {
        Self::new()
            .title(I18nKey::ConfirmDeleteSingleTitle.text())
            .message(I18nKey::ConfirmDeleteSingleMsg.text())
            .confirm_label(I18nKey::ConfirmDeleteLabel.text())
            .danger(true)
    }

    /// Batch delete confirmation for N selected items.
    pub fn delete_batch(count: usize) -> Self {
        Self::new()
            .title(I18nKey::ConfirmBatchTitle.text())
            .message(I18nKey::ConfirmBatchMsg.fmt(&[&count.to_string()]))
            .confirm_label(I18nKey::ConfirmDeleteLabel.text())
            .danger(true)
    }

    /// Remove app from hotkey blacklist confirmation.
    pub fn remove_blacklist(app_name: &str) -> Self {
        Self::new()
            .title(I18nKey::ConfirmRemoveTitle.text())
            .message(I18nKey::ConfirmRemoveMsg.fmt(&[app_name]))
            .confirm_label(I18nKey::ConfirmRemoveLabel.text())
            .danger(false)
    }

    /// Add app to hotkey blacklist confirmation.
    pub fn add_blacklist(app_name: &str) -> Self {
        Self::new()
            .title(I18nKey::ConfirmAddBlacklistTitle.text())
            .message(I18nKey::ConfirmAddBlacklistMsg.fmt(&[app_name]))
            .confirm_label(I18nKey::ConfirmAddLabel.text())
            .danger(false)
    }

    /// Add paste shortcut confirmation.
    pub fn add_paste_shortcut(app_name: &str, shortcut: &str) -> Self {
        let shortcut = crate::platform::hotkey::hotkey_display(shortcut);
        Self::new()
            .title(I18nKey::HotkeyPasteShortcutConfirmAddTitle.text())
            .message(I18nKey::HotkeyPasteShortcutConfirmAddMsg.fmt(&[app_name, &shortcut]))
            .confirm_label(I18nKey::ConfirmAddLabel.text())
            .danger(false)
    }

    /// Remove paste shortcut confirmation.
    pub fn remove_paste_shortcut(app_name: &str) -> Self {
        Self::new()
            .title(I18nKey::HotkeyPasteShortcutConfirmRemoveTitle.text())
            .message(I18nKey::HotkeyPasteShortcutConfirmRemoveMsg.fmt(&[app_name]))
            .confirm_label(I18nKey::ConfirmRemoveLabel.text())
            .danger(false)
    }

    /// Confirmation before adding an app to the clipboard app blacklist.
    pub fn add_clipboard_blacklist(app_name: &str) -> Self {
        Self::new()
            .title(I18nKey::ConfirmAddClipboardBlacklistTitle.text())
            .message(I18nKey::ConfirmAddClipboardBlacklistMsg.fmt(&[app_name]))
            .confirm_label(I18nKey::ConfirmAddLabel.text())
            .danger(false)
    }

    /// Confirmation before removing an app from the clipboard app blacklist.
    pub fn remove_clipboard_blacklist(app_name: &str) -> Self {
        Self::new()
            .title(I18nKey::ConfirmRemoveClipboardBlacklistTitle.text())
            .message(I18nKey::ConfirmRemoveClipboardBlacklistMsg.fmt(&[app_name]))
            .confirm_label(I18nKey::ConfirmRemoveLabel.text())
            .danger(false)
    }

    /// Tag deletion confirmation — explains that the tag is removed from all
    /// clipboard items while the items themselves are kept.
    pub fn delete_tag(tag_name: &str) -> Self {
        Self::new()
            .title(I18nKey::ConfirmDeleteTagTitle.text())
            .message(I18nKey::ConfirmDeleteTagMsg.fmt(&[tag_name]))
            .confirm_label(I18nKey::ConfirmDeleteLabel.text())
            .danger(true)
    }

    /// Sync backend deletion confirmation.
    pub fn delete_backend() -> Self {
        Self::new()
            .title(I18nKey::ConfirmDeleteSingleTitle.text())
            .message(I18nKey::BackendDeleteConfirmMsg.text())
            .confirm_label(I18nKey::ConfirmDeleteLabel.text())
            .danger(true)
    }
}

impl ConfirmDialog {
    /// Render the dialog with built-in enter animation (opacity + scale).
    ///
    /// `generation` should be bumped each time the dialog appears, so the
    /// animation restarts.  Use 0 to skip animation (dialog renders at full
    /// opacity / scale 1.0 immediately).
    pub fn render_animated(self, window: &mut Window, cx: &mut App, generation: u64) -> AnyElement {
        let Self {
            title,
            message,
            confirm_label,
            cancel_label,
            danger: is_danger,
            hold_seconds,
            on_confirm,
            on_cancel,
            options,
            focus_handle,
        } = self;
        // 冷静期未走完：确认钮禁用，原确认文案后接「（Ns）」括号秒数（同字体）。
        let holding = hold_seconds > 0;
        let confirm_label: SharedString = if holding {
            I18nKey::DialogHoldLabel
                .fmt(&[&confirm_label, &hold_seconds.to_string()])
                .into()
        } else {
            confirm_label.into()
        };

        let animating = generation != 0;
        let (opacity, scale) = if animating {
            let op = keyed_transition(
                window,
                cx,
                ("cd-opacity", generation),
                dialog_anim_duration(),
                0.0_f32,
                linear,
                1.0,
            );
            let sc = keyed_transition(
                window,
                cx,
                ("cd-scale", generation),
                dialog_anim_duration(),
                0.96_f32,
                linear,
                1.0,
            );
            (op, sc)
        } else {
            (1.0, 1.0)
        };

        let p = Palette::current(cx);

        if let Some(ref handle) = focus_handle {
            handle.focus(window, cx);
        }

        crate::ui::components::modal_overlay::modal_overlay()
            .rounded(Radius::Card.scaled())
            .flex()
            .items_center()
            .justify_center()
            .opacity(opacity)
            .when_some(focus_handle.as_ref(), |d, handle| d.track_focus(handle))
            .on_key_down({
                let on_confirm = on_confirm.clone();
                let on_cancel = on_cancel.clone();
                move |ev: &KeyDownEvent, window, cx| match ev.keystroke.key.as_str() {
                    "escape" => {
                        cx.stop_propagation();
                        if let Some(ref handler) = on_cancel {
                            handler(window, cx);
                        }
                    }
                    "enter" => {
                        cx.stop_propagation();
                        // 冷静期内 Enter 不放行：与禁用的确认钮口径一致。
                        if !holding {
                            if let Some(ref handler) = on_confirm {
                                handler(window, cx);
                            }
                        }
                    }
                    _ => {}
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let on_cancel = on_cancel.clone();
                move |_ev, _window, cx| {
                    cx.stop_propagation();
                    if let Some(ref handler) = on_cancel {
                        handler(_window, cx);
                    }
                }
            })
            .child(
                div()
                    .w(px(280. * scale))
                    .bg(p.flyout)
                    .rounded(Radius::Card.scaled())
                    .border(px(1.))
                    .border_color(p.stroke)
                    .p(px(16. * scale))
                    .occlude()
                    .child(
                        div()
                            .text_size(fs(16.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.ink)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(fs(12.))
                            .text_color(p.ink_2)
                            .mt(px(8.))
                            .child(message),
                    )
                    .when(!options.is_empty(), |dialog| {
                        dialog.child(div().mt(px(12.)).flex_col().children(
                            options.into_iter().map(|option| {
                                let on_toggle = option.on_toggle.clone();
                                let hover_bg = p.on_card("hover");
                                let control: SharedString =
                                    format!("confirm-option:{}", option.label).into();
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .py(px(4.))
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |row| row.rounded(px(4.)).bg(hover_bg))
                                    .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                                        cx.stop_propagation();
                                        on_toggle(window, cx);
                                    })
                                    .child(widgets::icon_moving(
                                        control.clone(),
                                        if option.selected {
                                            "CircleCheckIcon"
                                        } else {
                                            "CircleIcon"
                                        },
                                        fs(12.),
                                        if option.selected { p.accent } else { p.ink_3 },
                                    ))
                                    .child(widgets::icon_probe(control, CursorStyle::PointingHand))
                                    .child(
                                        div()
                                            .text_size(fs(12.))
                                            .text_color(p.ink)
                                            .child(option.label),
                                    )
                            }),
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_end()
                            .gap(px(8.))
                            .mt(px(16.))
                            .child({
                                let on_cancel = on_cancel.clone();
                                widgets::btn_small(
                                    &p,
                                    "confirm-dialog-cancel",
                                    cancel_label.clone(),
                                    None,
                                    widgets::Tone::Plain,
                                )
                                .on_click(move |_, window, cx| {
                                    if let Some(ref handler) = on_cancel {
                                        handler(window, cx);
                                    }
                                })
                            })
                            .child({
                                let on_confirm = on_confirm.clone();
                                widgets::btn_small(
                                    &p,
                                    "confirm-dialog-confirm",
                                    confirm_label.clone(),
                                    None,
                                    if is_danger {
                                        widgets::Tone::Danger
                                    } else {
                                        widgets::Tone::Accent
                                    },
                                )
                                .inert_dimmed(holding)
                                .on_click(move |_, window, cx| {
                                    if let Some(ref handler) = on_confirm {
                                        handler(window, cx);
                                    }
                                })
                            }),
                    ),
            )
            .into_any_element()
    }
}
