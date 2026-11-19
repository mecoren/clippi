//! Bottom status bar — displays the foreground application name, window title,
//! and provides quick actions (clipboard blacklist, hotkey blacklist).
//!
//! --- Matches the original Slint design: ---
//! --- - 26px height, 1px top border, divider color ---
//! --- - Left: app icon (16×16) + app name (12px) + window title (10px) ---
//! --- - Right: clipboard-blacklist + hotkey-blacklist icon buttons (22×22) ---

use gpui::prelude::*;
use gpui::*;

use crate::core::i18n_keys::I18nKey;
use crate::state::app::AppState;
use crate::ui::settings::hotkey;
use crate::ui::settings::SettingsEvent;
use crate::ui::settings::SettingsPanel;

use super::design::palette::Palette;
use super::design::size::{Ctrl, IconSize, Radius, Space};
use super::design::typography::{self, type_role};
use super::{design::tokens, widgets};

#[derive(IntoElement)]
pub struct BottomBar {
    state: Entity<AppState>,
    settings: Entity<SettingsPanel>,
}

impl BottomBar {
    pub fn new(state: Entity<AppState>, settings: Entity<SettingsPanel>) -> Self {
        Self { state, settings }
    }
}

impl RenderOnce for BottomBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let app_state = self.state.clone();
        let settings = self.settings.clone();
        let p = Palette::current(cx);

        let (fg_name, fg_title) = {
            let state = app_state.read(cx);
            (
                state.foreground_app_name.clone(),
                state.foreground_window_title.clone(),
            )
        };
        let has_fg = !fg_name.is_empty();
        let foreground_icon = has_fg
            .then(|| crate::core::paths::app_icon_path(&fg_name))
            .filter(|path| path.exists());

        div()
            .h(tokens::dimension("bottomBarHeight"))
            .w_full()
            .px(Space::Lg.scaled())
            .flex()
            .items_center()
            .justify_between()
            .border_t(px(1.))
            .border_color(p.stroke)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(Space::Md.scaled())
                    .min_w(px(0.))
                    .overflow_hidden()
                    .flex_1()
                    .when_some(foreground_icon, |row, image| {
                        row.child(
                            gpui::img(image)
                                .size(IconSize::Md.scaled())
                                .rounded(Radius::Sm.scaled())
                                .flex_shrink_0(),
                        )
                    })
                    .when(has_fg, |row| {
                        row.child(
                            div()
                                .flex()
                                .items_center()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .flex_1()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(typography::font(type_role::BODY))
                                        .font_weight(typography::LABEL_WEIGHT)
                                        .text_color(p.ink_2)
                                        .child(fg_name.clone()),
                                )
                                .when(!fg_title.is_empty(), |text| {
                                    text.child(
                                        div()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .flex_1()
                                            .text_size(typography::font(type_role::CAPTION))
                                            .text_color(p.ink_3)
                                            .child(format!(" \u{2014} {}", fg_title)),
                                    )
                                }),
                        )
                    })
                    .when(!has_fg, |row| {
                        row.child(
                            div()
                                .text_size(typography::font(type_role::CAPTION))
                                .text_color(p.ink_3)
                                .child(I18nKey::BottomBarNoApp.text()),
                        )
                    }),
            )
            .when(has_fg, |bar| {
                bar.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(Space::Xxs.scaled())
                        .child({
                            let fg = fg_name.clone();
                            let app_state = app_state.clone();
                            let settings = settings.clone();
                            let is_blacklisted = crate::core::settings::is_app_in_list(
                                &app_state.read(cx).settings.clipboard_app_blacklist,
                                &fg,
                            );

                            let base_color = if is_blacklisted { p.fav } else { p.ink_3 };
                            let tooltip_label = if is_blacklisted {
                                I18nKey::ConfirmRemoveClipboardBlacklistTitle.text()
                            } else {
                                I18nKey::BottomBarClipboardBlacklist.text()
                            };
                            div()
                                .id("bottom-clipboard-blacklist")
                                .role(Role::Button)
                                .tab_stop(true)
                                .aria_label(tooltip_label)
                                .relative()
                                .size(Ctrl::Chip.scaled())
                                .rounded(Radius::Md.scaled())
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor(CursorStyle::PointingHand)
                                .hover(move |style| style.bg(p.ink.opacity(0.06)))
                                .tooltip(move |window, cx| {
                                    widgets::tooltip(tooltip_label, window, cx)
                                })
                                .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                    cx.stop_propagation();
                                    if is_blacklisted {
                                        settings.update(cx, |_panel, cx| cx.emit(SettingsEvent::ShowHotkeyConfirm(hotkey::HotkeyConfirmAction::RemoveClipboardBlacklist { app_name: fg.clone() })));
                                    } else {
                                        settings.update(cx, |_panel, cx| cx.emit(SettingsEvent::ShowHotkeyConfirm(hotkey::HotkeyConfirmAction::AddClipboardBlacklist { app_name: fg.clone() })));
                                    }
                                })
                                .child(widgets::icon_moving(
                                    "bottom-clipboard-blacklist",
                                    "ClipboardBlockIcon",
                                    IconSize::Sm.scaled(),
                                    base_color,
                                ))
                                .child(widgets::icon_probe(
                                    "bottom-clipboard-blacklist",
                                    CursorStyle::PointingHand,
                                ))
                        })
                        .child({
                            let fg = fg_name.clone();
                            let app_state = app_state.clone();
                            let settings = settings.clone();
                            div()
                                .id("bottom-hotkey-blacklist")
                                .role(Role::Button)
                                .tab_stop(true)
                                .aria_label(I18nKey::BottomBarHotkeyBlacklist.text())
                                .relative()
                                .size(Ctrl::Chip.scaled())
                                .rounded(Radius::Md.scaled())
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor(CursorStyle::PointingHand)
                                .hover(move |style| style.bg(p.ink.opacity(0.06)))
                                .tooltip(|window, cx| {
                                    widgets::tooltip(
                                        I18nKey::BottomBarHotkeyBlacklist.text(),
                                        window,
                                        cx,
                                    )
                                })
                                .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                    cx.stop_propagation();
                                    if crate::core::settings::is_app_in_list(&app_state.read(cx).settings.hotkey_blacklist, &fg) {
                                        app_state.update(cx, |state, cx| { state.show_toast(I18nKey::BottomBarAlreadyInList.text()); cx.notify(); });
                                    } else {
                                        settings.update(cx, |_panel, cx| cx.emit(SettingsEvent::ShowHotkeyConfirm(hotkey::HotkeyConfirmAction::AddBlacklist { app_name: fg.clone() })));
                                    }
                                })
                                .child(widgets::icon_moving(
                                    "bottom-hotkey-blacklist",
                                    "BanIcon",
                                    IconSize::Sm.scaled(),
                                    p.ink_3,
                                ))
                                .child(widgets::icon_probe(
                                    "bottom-hotkey-blacklist",
                                    CursorStyle::PointingHand,
                                ))
                        })
                )
            })
    }
}
