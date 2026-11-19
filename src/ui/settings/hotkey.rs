//! Hotkey settings tab — recording + blacklist management + paste shortcuts.
//!
//! --- Layout (top to bottom): ---
//! --- - Hotkey recording card (66px): label + current hotkey button ---
//! --- - Foreground app info bar (44px): icon + app name + title + paste/blacklist buttons ---
//! --- - Blacklist section: label + scrollable list box ---
//! --- - Paste shortcut section: label + scrollable list box ---

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::widgets;

use super::SettingsPanel;
use crate::core::i18n_keys::I18nKey;
use crate::core::settings::LatestHotkeyEntry;
use crate::platform::hotkey::hotkey_display;
use crate::state::app::AppState;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Ctrl, Radius};
use crate::ui::font::fs;
use crate::ui::window_manager::{WinVTakeoverStatus, WindowManager};

/// Confirm action for hotkey settings operations.
/// Emitted by SettingsPanel, handled by RootView to show ConfirmDialog.
#[derive(Debug, Clone)]
pub enum HotkeyConfirmAction {
    /// Add app to hotkey blacklist.
    AddBlacklist {
        app_name: String,
    },
    /// Remove app from hotkey blacklist.
    RemoveBlacklist {
        app_name: String,
    },
    /// Add/update paste shortcut for an app.
    AddPasteShortcut {
        app_name: String,
        shortcut: String,
    },
    /// Remove paste shortcut for an app.
    RemovePasteShortcut {
        app_name: String,
    },
    AddClipboardBlacklist {
        app_name: String,
    },
    RemoveClipboardBlacklist {
        app_name: String,
    },
    /// Enable Win+V takeover (Windows only).
    WinVTakeoverEnable,
    /// Disable Win+V takeover (Windows only).
    WinVTakeoverDisable,
    /// Show manual setup instructions (Windows only).
    WinVManualSetup,
}

/// GPUI callback type alias for per-app list entries.
type AppCallback = Rc<dyn Fn(&mut Window, &mut App)>;
const LATEST_HOTKEY_POPUP_WIDTH: f32 = 304.;
const LATEST_HOTKEY_POPUP_HEIGHT: f32 = 316.;
const LATEST_HOTKEY_SLOT_WIDTH: f32 = 128.;
const LATEST_HOTKEY_COLUMN_GAP: f32 = 12.;

/// Optional presentation overrides for the shared recording card
/// (`render_recording_card`). `Default` reproduces the legacy rendering
/// element-for-element, so existing callers that pass `Default` keep their
/// current output unchanged (spec §5 共享组件边界).
#[derive(Clone, Copy, Default)]
struct RecordingCardOptions {
    /// Idle-state description shown in place of `HotkeyRecordingIdle`
    /// (AC-12). `None` keeps the legacy "点击开始录制" text.
    idle_desc: Option<I18nKey>,
    /// Render the clear affordance as a neutral (non-`p.danger`) text
    /// button ("重置"/"清空") to the LEFT of the record button, shown only
    /// while recording (AC-13). `false` keeps the legacy right-side
    /// `p.danger` red X. The clear callback semantics are unchanged in
    /// both modes.
    neutral_clear: bool,
}

impl SettingsPanel {
    /// Shared recording card: title + description + record button.
    /// Used for the main hotkey, quick hotkey and paste-plain hotkey cards.
    /// `on_clear` is an optional clear affordance shown only while recording —
    /// the idle card stays clean. `options` carries the optional AC-12/AC-13
    /// presentation overrides; passing `RecordingCardOptions::default()`
    /// renders exactly as before.
    #[allow(clippy::too_many_arguments)]
    fn render_recording_card(
        title: I18nKey,
        // 动效键的稳定前缀（与 i18n 文案无关，切换语言不会换 key）。
        control: &'static str,
        hotkey_display: SharedString,
        recording: bool,
        pending_single: Option<String>,
        p: &Palette,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
        on_clear: Option<AppCallback>,
        options: RecordingCardOptions,
    ) -> impl IntoElement {
        let waiting = pending_single.is_some();
        let waiting_color = p.warn;
        // The row sits inside a settings group, so its recording state shows as
        // a background tint rather than its own card chrome.
        let row_tint = if waiting {
            p.warn.opacity(0.13)
        } else {
            p.accent_soft()
        };
        let recording_btn_bg = if waiting {
            p.warn.opacity(0.13)
        } else if recording {
            p.accent_soft()
        } else {
            p.accent
        };
        let recording_btn_text = if waiting {
            waiting_color
        } else if recording {
            p.accent
        } else {
            p.on_accent
        };
        let desc_color = if waiting {
            waiting_color
        } else if recording {
            p.accent
        } else {
            p.ink_3
        };
        let desc_text = if waiting {
            I18nKey::HotkeySingleAwaiting.text()
        } else if recording {
            if cfg!(target_os = "windows") {
                I18nKey::HotkeyPressToRecordExtended.text()
            } else {
                I18nKey::HotkeyPressToRecord.text()
            }
        } else {
            // AC-12: optional idle-state description replaces the legacy
            // "点击开始录制" only when `idle_desc` is provided.
            options
                .idle_desc
                .unwrap_or(if cfg!(target_os = "windows") {
                    I18nKey::HotkeyRecordingIdleExtended
                } else {
                    I18nKey::HotkeyRecordingIdle
                })
                .text()
        };
        // Clear affordance (a text "重置" button for the paste-plain card) is
        // shown only while recording, so the idle card stays clean. It is
        // hidden during a pending single-key confirmation so it never competes
        // with the confirm affordance.
        let clear_btn = on_clear.filter(|_| recording && !waiting);

        div()
            .h(px(66.))
            .when(recording || waiting, |row| row.bg(row_tint))
            .px(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    // Leave room for the recording controls and extended hints.
                    .flex_1()
                    .min_w(px(0.))
                    .mr(px(8.))
                    .child(
                        div()
                            .text_size(fs(12.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.ink)
                            .child(title.text()),
                    )
                    .child(
                        div()
                            .text_size(fs(10.))
                            .text_color(desc_color)
                            .max_w_full()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(desc_text),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    // AC-13: neutral text clear button rendered to the LEFT of
                    // the record button when `neutral_clear` is set. Clicking it
                    // still runs the exact same clear callback.
                    .when(options.neutral_clear, |row| {
                        row.when_some(clear_btn.clone(), |d, clear| {
                            d.child(
                                widgets::btn_small(
                                    p,
                                    ElementId::Name(format!("hotkey-reset:{control}").into()),
                                    I18nKey::BtnReset.text(),
                                    None,
                                    widgets::Tone::Plain,
                                )
                                .on_click(move |_, window, cx| clear(window, cx)),
                            )
                        })
                    })
                    .child(
                        div()
                            .h(px(28.))
                            .w(px(80.))
                            .rounded(Radius::Md.scaled())
                            .bg(recording_btn_bg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(!recording || waiting, |d| {
                                d.cursor(CursorStyle::PointingHand).hover(move |style| {
                                    style.bg(widgets::hover_of(recording_btn_bg, p.ink))
                                })
                            })
                            .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                                if recording && !waiting {
                                    return;
                                }
                                on_click(window, cx);
                            })
                            .child(
                                div()
                                    .text_size(fs(12.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(recording_btn_text)
                                    .child(if pending_single.is_some() {
                                        I18nKey::HotkeyConfirmSingle.text().to_string()
                                    } else {
                                        hotkey_display.to_string()
                                    }),
                            ),
                    )
                    // Legacy clear (X) — right of the record button, red
                    // `p.danger` style; the default for existing callers.
                    .when(!options.neutral_clear, |row| {
                        row.when_some(clear_btn, |d, clear| {
                            let danger = p.danger;
                            let control: SharedString = format!("hotkey-clear:{control}").into();
                            d.child(
                                div()
                                    .relative()
                                    .w(Ctrl::Chip.scaled())
                                    .h(px(28.))
                                    .rounded(Radius::Md.scaled())
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |style| style.bg(danger.opacity(0.15)))
                                    .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                                        cx.stop_propagation();
                                        clear(window, cx);
                                    })
                                    .child(widgets::icon_moving(
                                        control.clone(),
                                        "CloseIcon",
                                        fs(9.),
                                        danger,
                                    ))
                                    .child(widgets::icon_probe(control, CursorStyle::PointingHand)),
                            )
                        })
                    }),
            )
    }

    /// Render a single latest-hotkey slot cell in the popup grid.
    #[allow(clippy::too_many_arguments)]
    fn render_latest_slot_cell(
        index: usize,
        latest: &[LatestHotkeyEntry],
        p: &Palette,
        recording: bool,
        on_record: AppCallback,
        on_clear: Option<AppCallback>,
    ) -> impl IntoElement {
        let entry = latest.get(index);
        let hotkey = entry.map(|e| e.hotkey.as_str()).unwrap_or("");
        let has_hotkey = entry.is_some_and(|e| !e.hotkey.is_empty());

        let label = if recording {
            I18nKey::LatestHotkeyRecording.text().to_string()
        } else if has_hotkey {
            hotkey_display(hotkey)
        } else {
            I18nKey::LatestHotkeyClickRecord.text().to_string()
        };
        let text_color = if has_hotkey || recording {
            p.accent
        } else {
            p.ink_3
        };
        let border_color = if has_hotkey || recording {
            p.accent
        } else {
            p.stroke
        };
        let bg_color = if has_hotkey || recording {
            p.accent_soft()
        } else {
            p.layer
        };
        let badge_bg = if has_hotkey || recording {
            p.accent
        } else {
            p.background
        };
        let badge_text = if has_hotkey || recording {
            p.on_accent
        } else {
            p.ink_3
        };
        let hover_bg = if has_hotkey || recording {
            p.accent_soft()
        } else {
            p.hover
        };
        let danger = p.danger;
        let rec = on_record.clone();

        div()
            .w(px(LATEST_HOTKEY_SLOT_WIDTH))
            .flex_shrink_0()
            .min_w(px(0.))
            .h(px(44.))
            .rounded(Radius::Md.scaled())
            .bg(bg_color)
            .border(px(if recording { 2. } else { 1. }))
            .border_color(border_color)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.bg(hover_bg))
            .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                rec(window, cx);
            })
            .child(
                div()
                    .w(px(20.))
                    .h(px(20.))
                    .rounded(px(10.))
                    .bg(badge_bg)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(fs(10.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(badge_text)
                            .child(format!("{}", index + 1)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(fs(12.))
                    .font_weight(if has_hotkey {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(text_color)
                    .child(label),
            )
            .when_some(on_clear.filter(|_| has_hotkey), move |d, clr| {
                let control: SharedString = format!("hotkey-latest-clear:{index}").into();
                d.child(
                    div()
                        .relative()
                        .w(Ctrl::Chip.scaled())
                        .h(Ctrl::Chip.scaled())
                        .rounded(Radius::Md.scaled())
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(danger.opacity(0.15)))
                        .on_mouse_down(MouseButton::Left, {
                            let clr = clr.clone();
                            move |_ev, window, cx| {
                                cx.stop_propagation();
                                clr(window, cx);
                            }
                        })
                        .child(widgets::icon_moving(
                            control.clone(),
                            "CloseIcon",
                            fs(9.),
                            danger,
                        ))
                        .child(widgets::icon_probe(control, CursorStyle::PointingHand)),
                )
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_latest_hotkeys_popup_overlay(
        panel_entity: Entity<SettingsPanel>,
        state: Entity<AppState>,
        wm: Entity<WindowManager>,
        latest: Vec<LatestHotkeyEntry>,
        recording_slot: Option<usize>,
        p: &Palette,
        layout: (f32, f32, f32, f32, f32),
    ) -> impl IntoElement {
        let (opacity, scale, offset, viewport_width, viewport_height) = layout;
        let popup_width = LATEST_HOTKEY_POPUP_WIDTH * scale;
        let popup_height = LATEST_HOTKEY_POPUP_HEIGHT * scale;
        let main_width = viewport_width.max(popup_width);
        let popup_left = ((main_width - popup_width) * 0.5).max(8.);
        let popup_top = ((viewport_height - popup_height) * 0.5).max(8.) + offset;
        let configured = latest
            .iter()
            .filter(|entry| !entry.hotkey.is_empty())
            .count();
        let close_panel = panel_entity.clone();

        crate::ui::components::modal_overlay::modal_overlay()
            .rounded(Radius::Card.scaled())
            .opacity(opacity)
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                close_panel.update(cx, |panel, cx| {
                    panel.latest_hotkeys_popup_open = false;
                    cx.notify();
                });
            })
            .child(
                div()
                    .absolute()
                    .left(px(popup_left))
                    .top(px(popup_top))
                    .w(px(popup_width))
                    .max_w(px(popup_width))
                    .rounded(px(8.))
                    .bg(p.flyout)
                    .border(px(1.))
                    .border_color(p.stroke)
                    .shadow_none()
                    .p(px(12. * scale))
                    .occlude()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .cursor(CursorStyle::Arrow)
                    .on_mouse_down(MouseButton::Left, |_ev, _window, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .text_size(fs(13.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.ink)
                                            .child(I18nKey::LatestHotkeysTitle.text()),
                                    )
                                    .child(
                                        div()
                                            .h(px(22.))
                                            .px(px(8.))
                                            .rounded(Radius::Pill.scaled())
                                            .bg(p.accent_soft())
                                            .border(px(1.))
                                            .border_color(p.accent)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(fs(10.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.accent)
                                            .child(format!("{}/10", configured)),
                                    ),
                            )
                            .child({
                                let close = panel_entity.clone();
                                let close_text = p.ink_2;
                                div()
                                    .relative()
                                    .w(Ctrl::Small.scaled())
                                    .h(Ctrl::Small.scaled())
                                    .rounded(Radius::Md.scaled())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |style| style.bg(p.ink.opacity(0.06)))
                                    .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                        cx.stop_propagation();
                                        close.update(cx, |panel, cx| {
                                            panel.latest_hotkeys_popup_open = false;
                                            cx.notify();
                                        });
                                    })
                                    .child(widgets::icon_moving(
                                        "hotkey-popup-close",
                                        "CloseIcon",
                                        fs(13.),
                                        close_text,
                                    ))
                                    .child(widgets::icon_probe(
                                        "hotkey-popup-close",
                                        CursorStyle::PointingHand,
                                    ))
                            }),
                    )
                    .child(div().h(px(1.)).bg(p.stroke))
                    .child(
                        div().w_full().flex().justify_center().child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .w(px(2. * LATEST_HOTKEY_SLOT_WIDTH + LATEST_HOTKEY_COLUMN_GAP))
                                .children((0..5).map(move |row| {
                                    let left = row;
                                    let right = row + 5;
                                    let latest_l = latest.clone();
                                    let latest_r = latest.clone();
                                    let wm_l = wm.clone();
                                    let wm_r = wm.clone();
                                    let panel_l = panel_entity.clone();
                                    let panel_r = panel_entity.clone();
                                    let state_l = state.clone();
                                    let state_r = state.clone();
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(LATEST_HOTKEY_COLUMN_GAP))
                                        .child({
                                            let panel_record = panel_l.clone();
                                            let rec: AppCallback = Rc::new(move |_window, cx| {
                                                wm_l.update(cx, |wm, cx| {
                                                    if wm.recording_latest_slot() != Some(left)
                                                        || !wm.confirm_pending_single_hotkey(cx)
                                                    {
                                                        wm.start_latest_slot_recording(left, cx);
                                                    }
                                                });
                                                panel_record.update(cx, |_panel, cx| cx.notify());
                                            });
                                            let panel = panel_l.clone();
                                            let state = state_l.clone();
                                            let wm_clear = wm.clone();
                                            let clr: AppCallback = Rc::new(move |_window, cx| {
                                                wm_clear.update(cx, |wm, _cx| {
                                                    wm.unregister_latest_slot_hotkey(left);
                                                });
                                                state.update(cx, |st, _cx| {
                                                    if left < st.settings.latest_hotkeys.len() {
                                                        st.settings.latest_hotkeys[left]
                                                            .hotkey
                                                            .clear();
                                                        st.settings.save();
                                                    }
                                                });
                                                panel.update(cx, |_panel, cx| cx.notify());
                                            });
                                            Self::render_latest_slot_cell(
                                                left,
                                                &latest_l,
                                                p,
                                                recording_slot == Some(left),
                                                rec,
                                                Some(clr),
                                            )
                                        })
                                        .child({
                                            let panel_record = panel_r.clone();
                                            let rec: AppCallback = Rc::new(move |_window, cx| {
                                                wm_r.update(cx, |wm, cx| {
                                                    if wm.recording_latest_slot() != Some(right)
                                                        || !wm.confirm_pending_single_hotkey(cx)
                                                    {
                                                        wm.start_latest_slot_recording(right, cx);
                                                    }
                                                });
                                                panel_record.update(cx, |_panel, cx| cx.notify());
                                            });
                                            let panel = panel_r.clone();
                                            let state = state_r.clone();
                                            let wm_clear = wm.clone();
                                            let clr: AppCallback = Rc::new(move |_window, cx| {
                                                wm_clear.update(cx, |wm, _cx| {
                                                    wm.unregister_latest_slot_hotkey(right);
                                                });
                                                state.update(cx, |st, _cx| {
                                                    if right < st.settings.latest_hotkeys.len() {
                                                        st.settings.latest_hotkeys[right]
                                                            .hotkey
                                                            .clear();
                                                        st.settings.save();
                                                    }
                                                });
                                                panel.update(cx, |_panel, cx| cx.notify());
                                            });
                                            Self::render_latest_slot_cell(
                                                right,
                                                &latest_r,
                                                p,
                                                recording_slot == Some(right),
                                                rec,
                                                Some(clr),
                                            )
                                        })
                                })),
                        ),
                    ),
            )
    }

    /// 「Alt 快捷切换」开关行。
    ///
    /// 抽成方法是为了让两个位置共用同一段渲染逻辑：Windows 放进「系统快捷键接管」
    /// 分组、其余平台放进全局快捷键分组末尾。
    fn render_alt_shortcuts_row(
        &mut self,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_toggle_row(
            I18nKey::SettingAltShortcuts,
            I18nKey::DescAltShortcutsOn,
            I18nKey::DescAltShortcutsOff,
            value,
            window,
            cx,
            |state, _this, _window, _cx| {
                state.update(_cx, |s, cx| {
                    s.settings.alt_shortcuts_enabled = !s.settings.alt_shortcuts_enabled;
                    s.settings.save();
                    // 关掉时立即清除按住 Alt 的状态，徽标随之消失。
                    s.set_alt_pressed(false);
                    cx.notify();
                });
            },
        )
        .into_any_element()
    }

    /// 「接管系统剪贴板快捷键」(Win+V) 行。
    ///
    /// 与其它设置行一样走分组内的裸行约定：`row_shell()` 提供行内边距，
    /// `setting_row_with_toggle()` 提供共享的动画开关，卡片边框与圆角由
    /// `settings_group` 统一提供。此前这一行自绘了 `rounded/bg/border` 小卡片，
    /// 嵌进分组卡片就成了双重描边、分割线风格与其它分组不一致。
    ///
    /// 可选的重检按钮与手动设置提示是这一行的第二段，跟在文字与开关下方：
    /// 左右内边距与 `row_shell()` 对齐（`px(14.)`），所以右边缘与开关齐平、
    /// 左边缘与标签齐平，不会看起来像另一张卡片。
    fn render_win_v_takeover_row(
        &mut self,
        takeover_active: bool,
        takeover_status: WinVTakeoverStatus,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status_text = match takeover_status {
            WinVTakeoverStatus::Active => I18nKey::WinVStatusActive.text(),
            WinVTakeoverStatus::HotkeyUnavailable => I18nKey::WinVStatusConflict.text(),
            WinVTakeoverStatus::RegistryUpdateRequired => I18nKey::WinVStatusUpdateRequired.text(),
            WinVTakeoverStatus::RegistryError => I18nKey::WinVStatusRegistryError.text(),
            WinVTakeoverStatus::Disabled => I18nKey::WinVTakeoverDesc.text(),
        };
        let show_recheck = takeover_status == WinVTakeoverStatus::HotkeyUnavailable;
        let show_manual = matches!(
            takeover_status,
            WinVTakeoverStatus::RegistryError | WinVTakeoverStatus::RegistryUpdateRequired
        );

        let p = self.palette;
        let text_3 = p.ink_3;

        let state_toggle = self.state.clone();
        let this_toggle = cx.entity().clone();
        let wm_recheck = self.window_manager.clone();
        let this_recheck = cx.entity().clone();
        let this_manual = cx.entity().clone();

        // 点击只发确认事件，真正的开关由 RootView 确认后再落盘。
        let on_toggle = move |_window: &mut Window, cx: &mut App| {
            let current = state_toggle.read(cx).settings.replace_system_win_v;
            let action = if current {
                HotkeyConfirmAction::WinVTakeoverDisable
            } else {
                HotkeyConfirmAction::WinVTakeoverEnable
            };
            this_toggle.update(cx, |_panel, cx| {
                cx.emit(super::SettingsEvent::ShowHotkeyConfirm(action));
            });
        };

        div()
            .flex()
            .flex_col()
            .child(self.setting_row_with_toggle(
                I18nKey::WinVTakeoverLabel.text(),
                status_text,
                takeover_active,
                on_toggle,
            ))
            .when(show_recheck || show_manual, move |row| {
                row.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .px(px(12.))
                        .pb(px(8.))
                        .when(show_recheck, move |row| {
                            row.child(
                                widgets::btn_small(
                                    &p,
                                    "win-v-recheck",
                                    I18nKey::WinVRecheckBtn.text(),
                                    None,
                                    widgets::Tone::Accent,
                                )
                                .on_click(move |_, _window, cx| {
                                    wm_recheck.update(cx, |wm, cx| {
                                        wm.recheck_win_v_takeover(cx);
                                    });
                                    this_recheck.update(cx, |_panel, cx| {
                                        cx.notify();
                                    });
                                }),
                            )
                        })
                        .when(show_manual, move |row| {
                            row.child(
                                div()
                                    .px(px(6.))
                                    .py(px(4.))
                                    .rounded(Radius::Md.scaled())
                                    .text_size(fs(10.))
                                    .text_color(text_3)
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |style| style.bg(p.ink.opacity(0.06)))
                                    .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                        this_manual.update(cx, |_panel, cx| {
                                            cx.emit(super::SettingsEvent::ShowHotkeyConfirm(
                                                HotkeyConfirmAction::WinVManualSetup,
                                            ));
                                        });
                                    })
                                    .child(I18nKey::WinVManualSetupHint.text()),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    pub fn render_hotkey_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self.state.clone();
        let wm = self.window_manager.clone();
        let this = cx.entity().clone();

        // Snapshot current values from AppState
        let app = self.state.read(cx);
        let main_hotkey_display = hotkey_display(&app.settings.hotkey);
        let recording = app.hotkey_recording;
        let pending_single = app.pending_single_hotkey.clone();
        let blacklist = app.settings.hotkey_blacklist.clone();
        let paste_shortcuts = app.settings.paste_shortcuts.clone();
        // 「快捷粘贴纯文本」hotkey: empty = disabled (spec §2). Snapshot both
        // the value (for the display + clear affordance) and the recording flag.
        let paste_plain_hotkey = app.settings.paste_plain_hotkey.clone();
        let paste_plain_display = hotkey_display(&paste_plain_hotkey);
        let has_paste_plain_hotkey = !paste_plain_hotkey.is_empty();
        let recording_paste_plain = app.recording_paste_plain_hotkey;
        let alt_shortcuts_enabled = app.settings.alt_shortcuts_enabled;
        #[cfg(target_os = "windows")]
        let _replace_system_win_v = app.settings.replace_system_win_v;
        // borrow released

        let takeover_status = wm.read(cx).win_v_takeover_status();
        let takeover_active =
            cfg!(target_os = "windows") && takeover_status != WinVTakeoverStatus::Disabled;

        let p = self.palette;

        let mut container = div().flex().flex_col().gap(px(12.)).pt(px(8.));

        // --- Global hotkeys ---
        let mut hotkey_rows: Vec<AnyElement> = vec![{
            let state = state.clone();
            let wm = wm.clone();
            let this = this.clone();

            #[cfg(target_os = "windows")]
            if takeover_active {
                // --- Managed by takeover: show "Win+V" fixed, disable recording ---
                let theme_text_1 = p.ink;
                let theme_text_3 = p.ink_3;
                div()
                    .h(px(66.))
                    .px(px(12.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(fs(12.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme_text_1)
                                    .child(I18nKey::HotkeyTabTitle.text()),
                            )
                            .child(
                                div()
                                    .text_size(fs(10.))
                                    .text_color(theme_text_3)
                                    .child(I18nKey::WinVManagedByMode.text()),
                            ),
                    )
                    .child(
                        div()
                            .h(px(28.))
                            .w(px(80.))
                            .rounded(Radius::Md.scaled())
                            .bg(p.accent_soft())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(fs(12.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.accent)
                                    .child("Win+V"),
                            ),
                    )
                    .into_any_element()
            } else {
                Self::render_recording_card(
                    I18nKey::HotkeyTabTitle,
                    "hotkey-main",
                    main_hotkey_display.clone().into(),
                    recording,
                    pending_single.clone().filter(|_| recording),
                    &p,
                    move |_window, cx| {
                        if state.read(cx).hotkey_recording
                            && state.read(cx).pending_single_hotkey.is_some()
                        {
                            wm.update(cx, |wm, cx| {
                                wm.confirm_pending_single_hotkey(cx);
                            });
                        } else {
                            wm.update(cx, |wm, cx| wm.start_hotkey_recording(cx));
                            state.update(cx, |s, _cx| s.hotkey_recording = true);
                        }
                        this.update(cx, |_panel, cx| cx.notify());
                    },
                    None,
                    RecordingCardOptions::default(),
                )
                .into_any_element()
            }

            #[cfg(not(target_os = "windows"))]
            {
                Self::render_recording_card(
                    I18nKey::HotkeyTabTitle,
                    "hotkey-main",
                    main_hotkey_display.clone().into(),
                    recording,
                    pending_single.clone().filter(|_| recording),
                    &p,
                    move |_window, cx| {
                        if state.read(cx).hotkey_recording
                            && state.read(cx).pending_single_hotkey.is_some()
                        {
                            wm.update(cx, |wm, cx| {
                                wm.confirm_pending_single_hotkey(cx);
                            });
                        } else {
                            wm.update(cx, |wm, cx| wm.start_hotkey_recording(cx));
                            state.update(cx, |s, _cx| s.hotkey_recording = true);
                        }
                        this.update(cx, |_panel, cx| cx.notify());
                    },
                    None,
                    RecordingCardOptions::default(),
                )
                .into_any_element()
            }
        }
        .into_any_element()];
        if self.state.read(cx).settings.quick_hotkey_enabled {
            // The conditional rows are written as row builders (closures) so
            // they can be pushed into the group's row list.
            let build_quick = {
                let quick_hotkey = hotkey_display(&self.state.read(cx).settings.quick_hotkey);
                let quick_recording = self.state.read(cx).recording_quick_hotkey;
                let pending_single = self.state.read(cx).pending_single_hotkey.clone();
                let state = state.clone();
                let wm = wm.clone();
                let this = this.clone();
                move |parent: Div| {
                    parent.child(Self::render_recording_card(
                        I18nKey::QuickHotkeyLabel,
                        "hotkey-quick",
                        quick_hotkey.into(),
                        quick_recording,
                        pending_single.filter(|_| quick_recording),
                        &p,
                        move |_window, cx| {
                            if state.read(cx).recording_quick_hotkey
                                && state.read(cx).pending_single_hotkey.is_some()
                            {
                                wm.update(cx, |wm, cx| {
                                    wm.confirm_pending_single_hotkey(cx);
                                });
                            } else {
                                wm.update(cx, |wm, cx| wm.start_quick_hotkey_recording(cx));
                                state.update(cx, |s, _cx| s.recording_quick_hotkey = true);
                            }
                            this.update(cx, |_panel, cx| cx.notify());
                        },
                        None,
                        RecordingCardOptions::default(),
                    ))
                }
            };
            hotkey_rows.push(build_quick(div()).into_any_element());
        }
        hotkey_rows.push(
            {
                let state = state.clone();
                let wm = wm.clone();
                let this = this.clone();
                // Empty state display mirrors the has_hotkey empty style
                // (muted "点击录制" label instead of a blank button).
                let display: SharedString = if has_paste_plain_hotkey {
                    paste_plain_display.clone().into()
                } else {
                    I18nKey::LatestHotkeyClickRecord.text().into()
                };
                let clear_cb: Option<AppCallback> = has_paste_plain_hotkey.then(|| {
                    let wm = wm.clone();
                    let this = this.clone();
                    let cb: AppCallback = Rc::new(move |_window, cx| {
                        wm.update(cx, |wm, cx| wm.clear_paste_plain_hotkey(cx));
                        this.update(cx, |_panel, cx| cx.notify());
                    });
                    cb
                });
                Self::render_recording_card(
                    I18nKey::HotkeyPastePlainLabel,
                    "hotkey-paste-plain",
                    display,
                    recording_paste_plain,
                    pending_single.clone().filter(|_| recording_paste_plain),
                    &p,
                    move |_window, cx| {
                        if state.read(cx).recording_paste_plain_hotkey
                            && state.read(cx).pending_single_hotkey.is_some()
                        {
                            wm.update(cx, |wm, cx| wm.confirm_pending_single_hotkey(cx));
                        } else {
                            state.update(cx, |s, _cx| s.recording_paste_plain_hotkey = true);
                            wm.update(cx, |wm, cx| wm.start_paste_plain_hotkey_recording(cx));
                        }
                        this.update(cx, |_panel, cx| cx.notify());
                    },
                    clear_cb,
                    RecordingCardOptions {
                        idle_desc: Some(I18nKey::HotkeyPastePlainDesc),
                        neutral_clear: true,
                    },
                )
            }
            .into_any_element(),
        );
        // 非 Windows 平台没有「系统快捷键接管」分组，把开关放在全局快捷键组末尾。
        #[cfg(not(target_os = "windows"))]
        hotkey_rows.push(self.render_alt_shortcuts_row(alt_shortcuts_enabled, _window, cx));
        container = container.child(self.settings_group(I18nKey::GroupHotkeys.text(), hotkey_rows));

        // --- System hotkey takeover: the group card holds the Win+V row (plus the
        // Alt row), so there is exactly one card with inset hairlines, like every
        // other settings group. ---
        if cfg!(target_os = "windows") {
            let win_v_rows: Vec<AnyElement> = vec![
                self.render_win_v_takeover_row(takeover_active, takeover_status, _window, cx),
                self.render_alt_shortcuts_row(alt_shortcuts_enabled, _window, cx),
            ];
            container =
                container.child(self.settings_group(I18nKey::GroupWinVTakeover.text(), win_v_rows));
        }

        // --- Lists & exclusions ---
        let mut list_rows: Vec<AnyElement> = vec![
            {
                let this_click = this.clone();
                let configured = self
                    .state
                    .read(cx)
                    .settings
                    .latest_hotkeys
                    .iter()
                    .filter(|e| !e.hotkey.is_empty())
                    .count();
                let desc = I18nKey::LatestHotkeysConfigured.fmt(&[&configured.to_string()]);
                self.setting_row_opener(
                    I18nKey::LatestHotkeysTitle.text(),
                    &desc,
                    move |_window, cx| {
                        this_click.update(cx, |panel, cx| {
                            let open = !panel.latest_hotkeys_popup_open;
                            panel.close_app_list_popups();
                            panel.latest_hotkeys_popup_open = open;
                            cx.notify();
                        });
                    },
                )
            }
            .into_any_element(),
            {
                let this_click = this.clone();
                let count = blacklist.len();
                let desc = I18nKey::ClipboardAppBlacklistCount.fmt(&[&count.to_string()]);
                self.setting_row_opener(
                    I18nKey::HotkeyBlacklist.text(),
                    &desc,
                    move |_window, cx| {
                        this_click.update(cx, |panel, cx| {
                            panel.toggle_hotkey_blacklist_popup();
                            cx.notify();
                        });
                    },
                )
            }
            .into_any_element(),
        ];
        if cfg!(target_os = "windows") {
            let build_paste_shortcuts = |root: Div| {
                let count = paste_shortcuts.len();
                let desc = I18nKey::ClipboardAppBlacklistCount.fmt(&[&count.to_string()]);
                let this_click = this.clone();
                root.child(self.setting_row_opener(
                    I18nKey::HotkeyPasteShortcut.text(),
                    &desc,
                    move |_window, cx| {
                        this_click.update(cx, |panel, cx| {
                            panel.toggle_paste_shortcuts_popup();
                            cx.notify();
                        });
                    },
                ))
            };
            list_rows.push(build_paste_shortcuts(div()).into_any_element());
        }
        container =
            container.child(self.settings_group(I18nKey::GroupHotkeyLists.text(), list_rows));

        container
    }
}
