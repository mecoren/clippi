//! Compact window chrome with animated shortcut hints.

use gpui::prelude::*;
use gpui::*;
use std::sync::{Arc, OnceLock};

use crate::core::i18n_keys::I18nKey;
use crate::state::app::AppState;

use super::clipboard_list::ClipboardListView;
use super::components::shortcut_hint::{shortcut_popover, shortcut_progress};
use super::design::palette::Palette;
use super::design::size::{Ctrl, IconSize, Radius, Space};
use super::design::typography::{self, type_role};
use super::theme::ClippiTheme;
use super::widgets;

/// Shared hit area and hover treatment for every titlebar action. Call sites
/// add `.aria_label(...)` so the accessible name matches the tooltip.
fn titlebar_button(id: &'static str, p: &Palette) -> Stateful<Div> {
    let p = *p;
    div()
        .id(id)
        .role(Role::Button)
        .tab_stop(true)
        // 图标动效探针覆盖整枚按钮，宿主必须 relative。
        .relative()
        .size(Ctrl::Input.scaled())
        .rounded(Radius::Md.scaled())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(p.hover))
}

/// In-app titlebar name: `Clippi`, or `Clippi (dev)` for a build that is not
/// from the release pipeline, so a local build is recognisable next to an
/// installed release.
///
/// The i18n value itself stays unprefixed — it doubles as the source-app name
/// stored for entries created in the editor (`platform::source::own_app_info`),
/// which must not change with the channel.
fn titlebar_app_name() -> String {
    let name = I18nKey::TitlebarAppName.text();
    if crate::core::channel::is_release() {
        name.to_string()
    } else {
        format!("{name} (dev)")
    }
}

fn titlebar_logo_image() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../assets/LOGO_notext.png").to_vec(),
        ))
    })
    .clone()
}

pub enum TitlebarEvent {
    TogglePin,
    ToggleTransfer,
    ToggleEmoji,
    OpenSettings,
}

pub struct Titlebar {
    state: Entity<AppState>,
    list_view: Entity<ClipboardListView>,
    pinned: bool,
    emoji_active: bool,
    alt_shortcuts_available: bool,
    emoji_shortcut_available: bool,
    hotkeys_shortcut_available: bool,
    transfer_shortcut_available: bool,
    theme: ClippiTheme,
}

impl Titlebar {
    pub fn new(
        state: Entity<AppState>,
        list_view: Entity<ClipboardListView>,
        theme: ClippiTheme,
    ) -> Self {
        Self {
            state,
            list_view,
            pinned: false,
            emoji_active: false,
            alt_shortcuts_available: false,
            emoji_shortcut_available: false,
            hotkeys_shortcut_available: false,
            transfer_shortcut_available: false,
            theme,
        }
    }

    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        self.pinned = pinned;
        cx.notify();
    }

    pub fn set_emoji_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.emoji_active != active {
            self.emoji_active = active;
            cx.notify();
        }
    }

    pub fn set_alt_shortcuts_available(
        &mut self,
        available: bool,
        emoji_available: bool,
        hotkeys_available: bool,
        transfer_available: bool,
        cx: &mut Context<Self>,
    ) {
        if self.alt_shortcuts_available != available
            || self.emoji_shortcut_available != emoji_available
            || self.hotkeys_shortcut_available != hotkeys_available
            || self.transfer_shortcut_available != transfer_available
        {
            self.alt_shortcuts_available = available;
            self.emoji_shortcut_available = emoji_available;
            self.hotkeys_shortcut_available = hotkeys_available;
            self.transfer_shortcut_available = transfer_available;
            cx.notify();
        }
    }

    pub fn set_theme(&mut self, theme: ClippiTheme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

impl EventEmitter<TitlebarEvent> for Titlebar {}

impl Render for Titlebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let (hotkeys_active, fav_active, show_hotkeys_filter, show_favorites_filter) = {
            let state = self.state.read(cx);
            let hotkeys_active = state.filters.is_hotkeys_active();
            let fav_active = state.filters.is_favorites_active();
            (
                hotkeys_active,
                fav_active,
                hotkeys_active || state.has_hotkey_items,
                fav_active || state.has_favorite_items,
            )
        };
        let state = self.state.read(cx);
        let alt_pressed = state.alt_pressed && state.settings.alt_shortcuts_enabled;
        let emoji_enabled = state.settings.emoji_panel_enabled;
        // Hiding the window leaves the fade transitions mid-flight (no final
        // frame is rendered), so key them by the hide generation: the next
        // show starts from settled instead of flashing a stale hint.
        let hint_generation = state.alt_hint_generation;
        let favorite_hint_progress = shortcut_progress(
            ("titlebar-favorites-shortcut", hint_generation),
            alt_pressed && self.alt_shortcuts_available && show_favorites_filter,
            window,
            cx,
        );
        let emoji_hint_progress = shortcut_progress(
            ("titlebar-emoji-shortcut", hint_generation),
            alt_pressed && self.emoji_shortcut_available && emoji_enabled,
            window,
            cx,
        );
        let (transfer_active, show_transfer_filter) = {
            let state = self.state.read(cx);
            (
                state.transfer_filter_active,
                state.settings.transfer_station_enabled,
            )
        };
        let hotkeys_hint_progress = shortcut_progress(
            ("titlebar-hotkeys-shortcut", hint_generation),
            alt_pressed && self.hotkeys_shortcut_available && show_hotkeys_filter,
            window,
            cx,
        );
        let transfer_hint_progress = shortcut_progress(
            ("titlebar-transfer-shortcut", hint_generation),
            alt_pressed && self.transfer_shortcut_available && show_transfer_filter,
            window,
            cx,
        );
        let pinned = self.pinned;
        let emoji_active = self.emoji_active;
        let emoji_titlebar = cx.entity().clone();
        let p = Palette::current(cx);
        let accent = p.accent;
        let text_2 = p.ink_2;
        let accent_soft = p.accent_soft();
        let fav_color = p.fav;
        let hotkey_state = self.state.clone();
        let hotkey_list_view = self.list_view.clone();
        let fav_state = self.state.clone();
        let fav_list_view = self.list_view.clone();
        let transfer_titlebar = cx.entity().clone();
        let hotkey_titlebar = cx.entity().clone();
        let fav_titlebar = cx.entity().clone();
        let pin_titlebar = cx.entity().clone();
        let settings_titlebar = cx.entity().clone();
        let logo = titlebar_logo_image();
        // DWM clips the rounded window on Windows, so the bar must stay square
        // or its own arc would show inside the system corner.
        let top_radius = if crate::core::frontend::SYSTEM_DRAWN_FRAME {
            0.
        } else {
            crate::core::frontend::PANEL_RADIUS
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(super::design::tokens::dimension("titlebarHeight"))
            .flex_shrink_0()
            .rounded_t(px(top_radius))
            .bg(p.background)
            // --- Left: logo + app name (also serves as drag area) ---
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(Space::Lg.scaled())
                    .pr(Space::Xs.scaled())
                    .gap(Space::Md.scaled())
                    .window_control_area(WindowControlArea::Drag)
                    // --- Logo (20x20, loaded from assets) ---
                    .child(gpui::img(logo).w(px(20.)).h(px(20.)))
                    // --- App name ---
                    .child(
                        div()
                            .text_size(typography::font(type_role::BODY))
                            .font_weight(typography::LABEL_WEIGHT)
                            .text_color(text_2)
                            .child(titlebar_app_name()),
                    ),
            )
            // --- Spacer ---
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            // --- Right: icon buttons ---
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Space::Xxs.scaled())
                    .pr(Space::Xs.scaled())
                    .when(emoji_enabled, |bar| {
                        bar.child(
                            div()
                                .relative()
                                .child(
                                    titlebar_button("titlebar-emoji", &p)
                                        .aria_label(I18nKey::EmojiPanel.text())
                                        .when(emoji_active, |button| button.bg(accent_soft))
                                        .tooltip(|window, cx| {
                                            widgets::tooltip(I18nKey::EmojiPanel.text(), window, cx)
                                        })
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            emoji_titlebar.update(cx, |_, cx| {
                                                cx.emit(TitlebarEvent::ToggleEmoji)
                                            });
                                        })
                                        .child(widgets::icon_moving(
                                            "titlebar-emoji",
                                            "FaceSmileIcon",
                                            IconSize::Md.scaled(),
                                            if emoji_active { accent } else { text_2 },
                                        ))
                                        .child(widgets::icon_probe(
                                            "titlebar-emoji",
                                            CursorStyle::PointingHand,
                                        )),
                                )
                                .when(
                                    self.emoji_shortcut_available && emoji_hint_progress > 0.001,
                                    |slot| {
                                        slot.child(shortcut_popover(
                                            "E",
                                            emoji_hint_progress,
                                            theme,
                                        ))
                                    },
                                ),
                        )
                    })
                    // --- Custom hotkey filter button (28x28) ---
                    .when(show_hotkeys_filter, |bar| {
                        bar.child(
                            div()
                                .relative()
                                .child(
                                    titlebar_button("titlebar-hotkeys-filter", &p)
                                        .aria_label(I18nKey::TitlebarTooltipHotkeys.text())
                                        .tooltip(|window, cx| {
                                            widgets::tooltip(
                                                I18nKey::TitlebarTooltipHotkeys.text(),
                                                window,
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_ev, _window, cx| {
                                                let items =
                                                    hotkey_state.update(cx, |state, _cx| {
                                                        state.toggle_hotkeys_filter();
                                                        state.visible_items()
                                                    });
                                                hotkey_list_view.update(cx, |list, cx| {
                                                    list.set_items(items, cx)
                                                });
                                                hotkey_titlebar
                                                    .update(cx, |_titlebar, cx| cx.notify());
                                            },
                                        )
                                        .child(widgets::icon_moving(
                                            "titlebar-hotkeys-filter",
                                            "KeyboardIcon",
                                            IconSize::Md.scaled(),
                                            if hotkeys_active { accent } else { text_2 },
                                        ))
                                        .child(widgets::icon_probe(
                                            "titlebar-hotkeys-filter",
                                            CursorStyle::PointingHand,
                                        )),
                                )
                                .when(
                                    self.hotkeys_shortcut_available
                                        && hotkeys_hint_progress > 0.001,
                                    |slot| {
                                        slot.child(shortcut_popover(
                                            "H",
                                            hotkeys_hint_progress,
                                            theme,
                                        ))
                                    },
                                ),
                        )
                    })
                    // --- Transfer station filter button (28x28) ---
                    .when(show_transfer_filter, |bar| {
                        bar.child(
                            div()
                                .relative()
                                .child(
                                    titlebar_button("titlebar-transfer-filter", &p)
                                        .aria_label(I18nKey::TransferStation.text())
                                        .tooltip(|window, cx| {
                                            widgets::tooltip(
                                                I18nKey::TransferStation.text(),
                                                window,
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_ev, _window, cx| {
                                                transfer_titlebar.update(cx, |_titlebar, cx| {
                                                    cx.emit(TitlebarEvent::ToggleTransfer);
                                                });
                                            },
                                        )
                                        .child(widgets::icon_moving(
                                            "titlebar-transfer-filter",
                                            "CloudIcon",
                                            IconSize::Md.scaled(),
                                            if transfer_active { accent } else { text_2 },
                                        ))
                                        .child(widgets::icon_probe(
                                            "titlebar-transfer-filter",
                                            CursorStyle::PointingHand,
                                        )),
                                )
                                .when(
                                    self.transfer_shortcut_available
                                        && transfer_hint_progress > 0.001,
                                    |slot| {
                                        slot.child(shortcut_popover(
                                            "T",
                                            transfer_hint_progress,
                                            theme,
                                        ))
                                    },
                                ),
                        )
                    })
                    // --- Fav filter button (28x28) ---
                    .when(show_favorites_filter, |bar| {
                        bar.child(
                            div()
                                .relative()
                                .child(
                                    titlebar_button("titlebar-favorites-filter", &p)
                                        .aria_label(I18nKey::TitlebarTooltipFavorites.text())
                                        .tooltip(|window, cx| {
                                            widgets::tooltip(
                                                I18nKey::TitlebarTooltipFavorites.text(),
                                                window,
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_ev, _window, cx| {
                                                let items = fav_state.update(cx, |state, _cx| {
                                                    state.toggle_favorites_filter();
                                                    state.visible_items()
                                                });
                                                fav_list_view.update(cx, |list, cx| {
                                                    list.set_items(items, cx)
                                                });
                                                fav_titlebar
                                                    .update(cx, |_titlebar, cx| cx.notify());
                                            },
                                        )
                                        .child(widgets::icon_moving(
                                            "titlebar-favorites-filter",
                                            "StarIcon",
                                            IconSize::Md.scaled(),
                                            if fav_active { fav_color } else { text_2 },
                                        ))
                                        .child(widgets::icon_probe(
                                            "titlebar-favorites-filter",
                                            CursorStyle::PointingHand,
                                        )),
                                )
                                .when(
                                    self.alt_shortcuts_available && favorite_hint_progress > 0.001,
                                    |slot| {
                                        slot.child(shortcut_popover(
                                            "D",
                                            favorite_hint_progress,
                                            theme,
                                        ))
                                    },
                                ),
                        )
                    })
                    // --- Pin button (28x28) ---
                    .child(
                        titlebar_button("titlebar-pin", &p)
                            .aria_label(if pinned {
                                I18nKey::TitlebarTooltipUnpin.text()
                            } else {
                                I18nKey::TitlebarTooltipPin.text()
                            })
                            .tooltip(move |window, cx| {
                                let label = if pinned {
                                    I18nKey::TitlebarTooltipUnpin.text()
                                } else {
                                    I18nKey::TitlebarTooltipPin.text()
                                };
                                widgets::tooltip(label, window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                pin_titlebar.update(cx, |_titlebar, cx| {
                                    cx.emit(TitlebarEvent::TogglePin);
                                });
                            })
                            .child(widgets::icon_moving(
                                "titlebar-pin",
                                "PinIcon",
                                IconSize::Md.scaled(),
                                if pinned { accent } else { text_2 },
                            ))
                            .child(widgets::icon_probe(
                                "titlebar-pin",
                                CursorStyle::PointingHand,
                            )),
                    )
                    // --- Settings button (28x28) ---
                    .child(
                        titlebar_button("titlebar-settings", &p)
                            .aria_label(I18nKey::SettingsTitle.text())
                            .tooltip(|window, cx| {
                                widgets::tooltip(I18nKey::SettingsTitle.text(), window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                settings_titlebar.update(cx, |_titlebar, cx| {
                                    cx.emit(TitlebarEvent::OpenSettings);
                                });
                            })
                            .child(widgets::icon_moving(
                                "titlebar-settings",
                                "SettingsIcon",
                                IconSize::Md.scaled(),
                                text_2,
                            ))
                            .child(widgets::icon_probe(
                                "titlebar-settings",
                                CursorStyle::PointingHand,
                            )),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::titlebar_logo_image;

    #[test]
    fn titlebar_logo_is_embedded_and_decodable() {
        let logo = titlebar_logo_image();
        assert_eq!(logo.format, gpui::ImageFormat::Png);
        assert!(image::load_from_memory(&logo.bytes).is_ok());
    }
}
