//! --- Settings panel — scrollable settings UI with tabs. ---
//!
//! --- Matches the original Slint `SettingsPanel.slint` layout: ---
//! --- - Top navigation bar: back button (→ icon) + "Settings" title (36px) ---
//! --- - Tab bar: 5 equal-width tabs (General/Clipboard/Hotkey/Data/Sync) ---
//!   with accent-green underline for active tab
//! --- - Scrollable content area routed by active tab index ---
//!
//! --- Individual settings controls will be added in follow-up work. ---
//! --- Tab rendering methods (`render_*_tab`) serve as extension points. ---

use crate::ui::font::fs;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::input::InputState;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarAxis, ScrollbarMode};

mod changelog;
mod clipboard;
mod data;
mod general;
pub mod hotkey;
mod sync;
mod version;

use data::LocalBackupDialog;
use data::ResetDataDirState;
use hotkey::HotkeyConfirmAction;

use crate::core::i18n_keys::I18nKey;
use crate::state::app::AppState;
use crate::ui::add_backend::AddBackendPanel;
use crate::ui::components::confirm_dialog::ConfirmDialog;
use crate::ui::components::slider::{
    SliderDetent, SliderDragState, SteppedSlider, SteppedSliderColors,
};
use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Ctrl, IconSize, Radius, Space};
use crate::ui::design::typography::{self, type_role};
use crate::ui::theme::ClippiTheme;
use crate::ui::widgets;
use crate::ui::window_manager::WindowManager;

/// Events emitted by the settings panel.
pub enum SettingsEvent {
    /// User clicked the back button — return to clipboard view.
    Back,
    /// Active tab changed — RootView should clear update toast when
    /// switching to the version tab (index 5).
    TabChanged(usize),
    /// Theme setting changed — RootView should rebuild its ClippiTheme.
    ThemeChanged,
    ClipboardSettingsChanged {
        reload_items: bool,
        scroll_to_top: bool,
    },
    /// User clicked add/remove blacklist — RootView should show a ConfirmDialog.
    ShowHotkeyConfirm(HotkeyConfirmAction),
    /// User confirmed add/remove paste shortcut — RootView should apply changes.
    HotkeyPasteShortcut { action: HotkeyConfirmAction },
    /// Data settings error — RootView should show a toast.
    DataError(String),
    /// Data settings info toast — no error prefix.
    DataToast(String),
    /// Show the clear-data confirmation at the RootView overlay level.
    ShowClearDataConfirm,
    /// Font scale or family changed — RootView should recompute cached card
    /// heights (they bake in the live scale) and re-render the whole tree.
    FontChanged,
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

/// The settings panel entity.
pub struct SettingsPanel {
    active_tab: usize,
    state: Entity<AppState>,
    window_manager: Entity<WindowManager>,
    theme: ClippiTheme,
    /// 语义色板：设置页共享层（分组、行、开关、选项）唯一取色入口。
    /// 与 `theme` 并存是因为子页仍有大量 `ClippiTheme` 字段在用，逐页迁移；
    /// 每帧 `render` 开头刷新，保证主题切换后当帧即生效。
    palette: Palette,
    scroll_handle: ScrollHandle,
    backend_collapse_states: HashMap<String, BackendCollapseState>,
    /// Tab 内容进场（Argus `enter / hold_entrance / page_entering` 的载体）。
    page_entrance: widgets::PageEntrance,
    backend_panel: Entity<AddBackendPanel>,
    /// Whether the bundled changelog page is open in place of the tab content.
    pub changelog_open: bool,
    /// Whether the latest hotkeys popup is open.
    pub latest_hotkeys_popup_open: bool,
    /// Application list popup visibility (hotkey blacklist / paste shortcuts / clipboard blacklist).
    pub hotkey_blacklist_popup_open: bool,
    pub paste_shortcuts_popup_open: bool,
    pub app_blacklist_popup_open: bool,
    pub privacy_types_popup_open: bool,
    /// Pending hotkey blacklist confirmation (consumed by RootView).
    pub hotkey_confirm: Option<HotkeyConfirmAction>,
    /// Whether we are currently recording a paste shortcut for an app (Some(app_name)).
    pub recording_paste_shortcut: Option<String>,
    /// The recorded paste shortcut string before confirmation (app_name, shortcut).
    pub pending_paste_shortcut: Option<(String, String)>,
    /// Reset-data-directory dialog state (portable mode only).
    pub reset_data_dialog: Option<ResetDataDirState>,
    /// Pending backend deletion confirmation (backend id).
    pub delete_backend_confirm: Option<String>,
    delete_backend_confirm_gen: u64,
    delete_backend_confirm_started: Option<Instant>,
    /// Whether the max-items field is in editing mode.
    editing_max_items: bool,
    /// Input entity for the max-items editor (created once in constructor).
    max_items_input: Entity<InputState>,
    /// Pending async file dialog for changing the database path.
    _db_path_dialog_task: Option<Task<()>>,
    local_backup_busy: bool,
    pub local_backup_dialog: Option<LocalBackupDialog>,
    local_backup_dialog_generation: u64,
    _local_backup_task: Option<Task<()>>,
    /// Focus-out subscription for the max-items input (auto-save on blur).
    _max_items_focus_sub: gpui::Subscription,
    /// Whether the retention-days field is in editing mode.
    editing_retention_days: bool,
    /// Input entity for the retention-days editor (created once in constructor).
    retention_days_input: Entity<InputState>,
    /// Focus-out subscription for the retention-days input (auto-save on blur).
    _retention_days_focus_sub: gpui::Subscription,
    /// Animation generation counter for copy-sound card expand/collapse.
    pub copy_sound_anim_gen: u64,
    /// Focus handle for keyboard events (ESC to go back).
    focus_handle: FocusHandle,

    // ── Config sync ──
    /// Selected backend ID for config sync (defaults to first available).
    pub config_sync_backend_id: Option<String>,
    /// Upload confirmation dialog state.
    pub config_sync_upload_confirm: Option<String>,
    /// Upload confirmation dialog generation (incremented on each new dialog).
    pub config_sync_upload_confirm_gen: u64,
    /// Apply confirmation dialog generation (incremented on each new dialog).
    pub config_sync_apply_confirm_gen: u64,
    /// When the apply confirmation was shown.
    pub config_sync_apply_confirm_started: Option<Instant>,
    /// Whether the config-sync backend selector dropdown is open.
    pub config_sync_menu_open: bool,

    // ── Font picker ──
    /// Whether the custom font-family picker overlay is open.
    pub font_picker_open: bool,
    /// Lazily-enumerated installed font families (populated the first time the
    /// picker opens so we never re-scan the system font list on every render).
    available_fonts: Option<Rc<Vec<String>>>,
    /// Caller-owned drag state per slider id, so each slider's press/drag
    /// survives the re-renders every detent commit triggers.
    slider_drags: HashMap<&'static str, SliderDragState>,
    /// Frames left to align the font list with the current font after opening.
    /// The alignment needs the list's measured row bounds, which are only
    /// available once it has been laid out, so it is retried for a few frames
    /// and then stops (a settled list must not fight the user's scrolling).
    font_picker_scroll_frames: u8,
}

fn tab_names() -> [&'static str; 6] {
    [
        I18nKey::TabGeneral.text(),
        I18nKey::TabClipboard.text(),
        I18nKey::TabHotkey.text(),
        I18nKey::TabData.text(),
        I18nKey::TabSync.text(),
        I18nKey::TabVersion.text(),
    ]
}

#[derive(Clone, Copy)]
pub(crate) struct BackendCollapseState {
    pub enabled: bool,
    pub generation: u64,
}

impl SettingsPanel {
    pub fn new(
        state: Entity<AppState>,
        window_manager: Entity<WindowManager>,
        theme: ClippiTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let max_items_input = cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx));
        let retention_days_input =
            cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx));
        let backend_panel = cx.new(|cx| AddBackendPanel::new(window_manager.clone(), window, cx));

        // --- Subscribe to focus-out on the max-items InputState. ---
        // --- When the input loses focus, save and exit editing. ---
        let state_sub = state.clone();
        let input_sub = max_items_input.clone();
        let handle = max_items_input.read(cx).focus_handle(cx);
        let _max_items_focus_sub =
            cx.on_focus_out(&handle, window, move |this, _ev, _window, cx| {
                if this.editing_max_items {
                    let text = input_sub.read(cx).value().to_string();
                    let n: u32 = text.trim().parse().unwrap_or(0);
                    state_sub.update(cx, |s, _cx| {
                        s.settings.max_items = n;
                        s.settings.save();
                    });
                    this.editing_max_items = false;
                    cx.notify();
                }
            });

        // --- Subscribe to focus-out on the retention-days InputState. ---
        let state_sub_rd = state.clone();
        let input_sub_rd = retention_days_input.clone();
        let handle_rd = retention_days_input.read(cx).focus_handle(cx);
        let _retention_days_focus_sub =
            cx.on_focus_out(&handle_rd, window, move |this, _ev, _window, cx| {
                if this.editing_retention_days {
                    let text = input_sub_rd.read(cx).value().to_string();
                    let n: u32 = text.trim().parse().unwrap_or(0);
                    state_sub_rd.update(cx, |s, _cx| {
                        s.settings.retention_days = n;
                        s.settings.save();
                    });
                    this.editing_retention_days = false;
                    cx.notify();
                }
            });

        Self {
            active_tab: 0,
            state,
            window_manager,
            theme,
            palette: Palette::current(cx),
            scroll_handle: ScrollHandle::default(),
            backend_collapse_states: HashMap::new(),
            page_entrance: widgets::PageEntrance::settled(),
            focus_handle: cx.focus_handle(),
            backend_panel,
            changelog_open: false,
            latest_hotkeys_popup_open: false,
            hotkey_blacklist_popup_open: false,
            paste_shortcuts_popup_open: false,
            app_blacklist_popup_open: false,
            privacy_types_popup_open: false,
            hotkey_confirm: None,
            recording_paste_shortcut: None,
            pending_paste_shortcut: None,
            reset_data_dialog: None,
            delete_backend_confirm: None,
            delete_backend_confirm_gen: 0,
            delete_backend_confirm_started: None,
            editing_max_items: false,
            max_items_input,
            _db_path_dialog_task: None,
            local_backup_busy: false,
            local_backup_dialog: None,
            local_backup_dialog_generation: 0,
            _local_backup_task: None,
            _max_items_focus_sub,
            editing_retention_days: false,
            retention_days_input,
            _retention_days_focus_sub,
            copy_sound_anim_gen: 0,
            config_sync_backend_id: None,
            config_sync_upload_confirm: None,
            config_sync_upload_confirm_gen: 0,
            config_sync_apply_confirm_gen: 0,
            config_sync_apply_confirm_started: None,
            config_sync_menu_open: false,
            font_picker_open: false,
            available_fonts: None,
            slider_drags: HashMap::new(),
            font_picker_scroll_frames: 0,
        }
    }

    /// Reload theme from the computed ClippiTheme (called by RootView after ThemeChanged).
    pub fn reload_theme(&mut self, theme: ClippiTheme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Switch to a specific tab by index.
    pub fn set_active_tab(&mut self, index: usize) {
        // The changelog is a sub-page of the version tab; any tab navigation
        // leaves it, including reopening settings straight on the version tab.
        self.changelog_open = false;
        if self.active_tab != index {
            self.local_backup_dialog = None;
            self.local_backup_dialog_generation =
                self.local_backup_dialog_generation.wrapping_add(1);
            self.active_tab = index;
            self.page_entrance.arm();
        }
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn app_list_popup_open(&self) -> bool {
        self.local_backup_dialog.is_some()
            || self.privacy_types_popup_open
            || self.app_blacklist_popup_open
            || self.hotkey_blacklist_popup_open
            || self.paste_shortcuts_popup_open
            || self.latest_hotkeys_popup_open
    }

    pub fn close_app_list_popups(&mut self) {
        self.local_backup_dialog = None;
        self.local_backup_dialog_generation = self.local_backup_dialog_generation.wrapping_add(1);
        self.hotkey_blacklist_popup_open = false;
        self.paste_shortcuts_popup_open = false;
        self.app_blacklist_popup_open = false;
        self.privacy_types_popup_open = false;
    }

    pub fn toggle_hotkey_blacklist_popup(&mut self) {
        let open = !self.hotkey_blacklist_popup_open;
        self.close_app_list_popups();
        self.latest_hotkeys_popup_open = false;
        self.hotkey_blacklist_popup_open = open;
    }

    pub fn toggle_paste_shortcuts_popup(&mut self) {
        let open = !self.paste_shortcuts_popup_open;
        self.close_app_list_popups();
        self.latest_hotkeys_popup_open = false;
        self.paste_shortcuts_popup_open = open;
    }

    pub fn toggle_app_blacklist_popup(&mut self) {
        let open = !self.app_blacklist_popup_open;
        self.close_app_list_popups();
        self.latest_hotkeys_popup_open = false;
        self.app_blacklist_popup_open = open;
    }

    pub fn backend_panel(&self) -> Entity<AddBackendPanel> {
        self.backend_panel.clone()
    }

    /// Render config-sync confirmation dialogs as absolute-positioned overlays
    /// on the settings panel root. This avoids clipping from the scroll container.
    pub(crate) fn render_config_sync_dialogs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wm = self.window_manager.clone();
        let this = cx.entity().clone();
        let theme = self.theme.clone();

        let show_upload = self.config_sync_upload_confirm.is_some();
        let show_apply = wm.read(cx).config_sync_pending_snapshot().is_some();
        // Bump the animation generation when the apply dialog appears so it
        // plays the same enter animation as the other confirm dialogs.
        if show_apply {
            if self.config_sync_apply_confirm_started.is_none() {
                self.config_sync_apply_confirm_gen =
                    self.config_sync_apply_confirm_gen.wrapping_add(1);
                self.config_sync_apply_confirm_started = Some(Instant::now());
            }
        } else {
            self.config_sync_apply_confirm_started = None;
        }

        div()
            .when(show_upload || show_apply, |root| {
                // Positioning context only — ConfirmDialog provides its own
                // backdrop, so no second dimming layer is drawn here.
                root.absolute().size_full().top_0().left_0()
            })
            .when(show_upload, |root| {
                let backend_name = self.config_sync_upload_confirm.clone().unwrap_or_default();
                let msg = I18nKey::ConfigSyncConfirmUploadMsg.fmt(&[&backend_name]);
                let backend_id = self.config_sync_backend_id.clone();
                let wm = wm.clone();
                let this = this.clone();
                let theme = theme.clone();

                root.child(
                    ConfirmDialog::new()
                        .title(I18nKey::ConfigSyncConfirmUploadTitle.text())
                        .message(msg)
                        .confirm_label(I18nKey::BtnApply.text())
                        .danger(false)
                        .theme(theme)
                        .on_confirm({
                            let wm = wm.clone();
                            let this = this.clone();
                            let bid = backend_id.clone();
                            move |_window, app| {
                                let backends = this.read(app).state.read(app).settings.sync_backends.clone();
                                if let Some(ref id) = bid {
                                    if let Some(cfg) = backends.iter().find(|b| &b.id == id) {
                                        if let Some(backend) = crate::services::backends::create_config_snapshot_backend(cfg) {
                                            wm.update(app, |wm, cx| {
                                                wm.start_config_upload(backend, cx);
                                            });
                                        }
                                    }
                                }
                                this.update(app, |panel, cx| {
                                    panel.config_sync_upload_confirm = None;
                                    cx.notify();
                                });
                            }
                        })
                        .on_cancel({
                            let this = this.clone();
                            move |_window, app| {
                                this.update(app, |panel, cx| {
                                    panel.config_sync_upload_confirm = None;
                                    cx.notify();
                                });
                            }
                        })
                        .render_animated(window, cx, self.config_sync_upload_confirm_gen),
                )
            })
            .when(show_apply, |root| {
                let wm = wm.clone();
                let this = this.clone();
                let theme = theme.clone();
                let snapshot = wm.read(cx).config_sync_pending_snapshot().cloned();

                if let Some(ref snap) = snapshot {
                    let uploaded = format_uploaded_at(&snap.uploaded_at);
                    let platform = snap.source.platform.clone();
                    let version = snap.source.app_version.clone();
                    let msg = I18nKey::ConfigSyncConfirmApplyMsg.fmt(&[&uploaded, &platform, &version]);
                    let gen = self.config_sync_apply_confirm_gen;

                    root.child(
                        ConfirmDialog::new()
                            .title(I18nKey::ConfigSyncConfirmApplyTitle.text())
                            .message(msg)
                            .confirm_label(I18nKey::BtnRestartNow.text())
                            .danger(false)
                            .theme(theme)
                            .on_confirm({
                                let wm = wm.clone();
                                move |_window, app| {
                                    wm.update(app, |wm, cx| {
                                        wm.apply_config_snapshot(cx);
                                    });
                                }
                            })
                            .on_cancel({
                                let wm = wm.clone();
                                let this = this.clone();
                                move |_window, app| {
                                    wm.update(app, |wm, _cx| {
                                        wm.clear_config_sync_pending_snapshot();
                                    });
                                    this.update(app, |_panel, cx| {
                                        cx.notify();
                                    });
                                }
                            })
                            .render_animated(window, cx, gen),
                    )
                } else {
                    root.child(div())
                }
            })
    }

    /// Render delete-backend confirmation dialog as an absolute overlay.
    pub(crate) fn render_delete_backend_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let show = self.delete_backend_confirm.is_some();
        let gen = if show {
            self.delete_backend_confirm_gen
        } else {
            0
        };
        let wm = self.window_manager.clone();
        let this = cx.entity().clone();
        let theme = self.theme.clone();

        div().when(show, |root| {
            let id = self.delete_backend_confirm.clone().unwrap_or_default();
            // Positioning context only — ConfirmDialog provides its own
            // backdrop, so no second dimming layer is drawn here.
            root.absolute().size_full().top_0().left_0().child(
                ConfirmDialog::delete_backend()
                    .theme(theme)
                    .on_confirm({
                        let wm = wm.clone();
                        let this = this.clone();
                        move |_window, app| {
                            wm.update(app, |wm, cx| {
                                wm.remove_sync_backend(&id, cx);
                            });
                            this.update(app, |panel, cx| {
                                panel.delete_backend_confirm = None;
                                cx.notify();
                            });
                        }
                    })
                    .on_cancel({
                        let this = this.clone();
                        move |_window, app| {
                            this.update(app, |panel, cx| {
                                panel.delete_backend_confirm = None;
                                cx.notify();
                            });
                        }
                    })
                    .render_animated(window, cx, gen),
            )
        })
    }
}

/// Format an RFC3339 timestamp as local date-time.
fn format_uploaded_at(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_string())
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 主题可能在两次渲染之间被外部换掉（设置里切主题/强调色），
        // 共享层构造器只认这份色板，所以每帧先刷新一次。
        self.palette = Palette::current(cx);
        let active = self.active_tab;
        let changelog_open = self.changelog_open;
        let theme = &self.theme;
        let this = cx.entity().clone();
        let focus_handle = self.focus_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .rounded_b(px(12.))
            .bg(theme.bg)
            .track_focus(&focus_handle)
            //  Navigation bar (height 36px, mt 8px matching Slint y=8px)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(8.))
                    .mt(px(4.))
                    // --- Back button (28x28, Lucide) ---
                    .child(
                        div()
                            .w(px(28.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor(CursorStyle::PointingHand)
                            .on_mouse_down(MouseButton::Left, {
                                let this = this.clone();
                                move |_ev, _window, cx| {
                                    this.update(cx, |panel, cx| {
                                        // The changelog is a sub-page: the same
                                        // arrow first returns to the version tab.
                                        if panel.changelog_open {
                                            panel.close_changelog(cx);
                                        } else {
                                            cx.emit(SettingsEvent::Back);
                                        }
                                    });
                                }
                            })
                            .child(widgets::icon("BackIcon", fs(16.), Hsla::from(theme.text_2))),
                    )
                    // --- Title（TITLE 档 16px，700 weight，text_1） ---
                    .child(
                        div()
                            .text_size(fs(16.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text_1)
                            .child(if changelog_open {
                                I18nKey::Changelog.text()
                            } else {
                                I18nKey::SettingsTitle.text()
                            }),
                    ),
            )
            //  Tab bar (height 36px, mt 8px matching Slint spacing). Hidden on
            //  the changelog sub-page, which owns the panel below the title.
            .when(!changelog_open, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .h(px(36.))
                        .px(px(8.))
                        .mt(px(2.))
                        .border_b(px(1.))
                        .border_color(theme.divider)
                        .children(tab_names().iter().enumerate().map(|(i, name)| {
                            let is_active = i == active;
                            let tab_color = if is_active {
                                theme.accent
                            } else {
                                theme.text_2
                            };
                            let underline_bg = if is_active {
                                theme.accent
                            } else {
                                Rgba::from(Hsla::transparent_black())
                            };
                            let this = this.clone();

                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .h_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .cursor(CursorStyle::PointingHand)
                                .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                    this.update(cx, |panel, cx| {
                                        panel.set_active_tab(i);
                                        panel.close_app_list_popups();
                                        panel.latest_hotkeys_popup_open = false;
                                        panel.font_picker_open = false;
                                        cx.emit(SettingsEvent::TabChanged(i));
                                        cx.notify();
                                    });
                                })
                                // --- Tab label ---
                                .child(
                                    div()
                                        .text_size(fs(12.))
                                        .font_weight(if is_active {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::default()
                                        })
                                        .text_color(tab_color)
                                        .child(*name),
                                )
                                // --- Active underline indicator (2px) ---
                                .child(div().w_full().h(px(2.)).mt(px(4.)).bg(underline_bg))
                        })),
                )
            })
            // --- Tab content (fills remaining space, scrollable) ---
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded_b(px(12.))
                    .bg(theme.bg)
                    .pt(px(8.))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .w_full()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("settings-scroll-area")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll_handle)
                                    .child({
                                        let content: AnyElement = if changelog_open {
                                            self.render_changelog(window, cx).into_any_element()
                                        } else {
                                            match active {
                                                0 => self
                                                    .render_general_tab(window, cx)
                                                    .into_any_element(),
                                                1 => self
                                                    .render_clipboard_tab(window, cx)
                                                    .into_any_element(),
                                                2 => self
                                                    .render_hotkey_tab(window, cx)
                                                    .into_any_element(),
                                                3 => self
                                                    .render_data_tab(window, cx)
                                                    .into_any_element(),
                                                4 => self
                                                    .render_sync_tab(window, cx)
                                                    .into_any_element(),
                                                5 => self
                                                    .render_version_tab(window, cx)
                                                    .into_any_element(),
                                                _ => div().into_any_element(),
                                            }
                                        };
                                        div()
                                            .w_full()
                                            .flex()
                                            .flex_col()
                                            .px(px(8.))
                                            // 页脚固定悬浮（44 高），留白 = 24+24+8：
                                            // 最后一行停在页脚之上，且全为 4pt 档位。
                                            .when(active != 5 || changelog_open, |el| {
                                                el.pb(Space::Xxl.scaled()
                                                    + Space::Xxl.scaled()
                                                    + Space::Md.scaled())
                                            })
                                            .child(widgets::enter(
                                                &mut self.page_entrance,
                                                content,
                                                widgets::EntranceAxis::Vertical,
                                                window,
                                                cx,
                                            ))
                                    }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(4.))
                                    .right(px(0.))
                                    .bottom(px(10.))
                                    .w(px(16.))
                                    .child(
                                        Scrollbar::vertical(&self.scroll_handle)
                                            .mode(ScrollbarMode::Scrolling),
                                    ),
                            ),
                    ),
            )
    }
}

// --- Reusable control helpers ---

impl SettingsPanel {
    /// 卡片展开/收起这类高度与不透明度的帧间过渡（时长与曲线取动效令牌）。
    fn transition_f32(
        window: &mut Window,
        cx: &mut Context<Self>,
        key: (&'static str, u64),
        initial: f32,
        target: f32,
    ) -> f32 {
        crate::ui::transition::keyed_transition(
            window,
            cx,
            key,
            motion::fast(),
            initial,
            motion::ease_out,
            target,
        )
    }

    /// A labelled group of settings rows.
    ///
    /// The section title sits above a single card that holds every row of the
    /// group, with hairline dividers between rows — so related settings read as
    /// one block instead of a wall of separate cards. Rows are rendered without
    /// their own card chrome (see the `setting_row_*` helpers).
    ///
    /// A group holding a single row is self-evident, so it is rendered without a
    /// title (the row's own label already names it).
    pub(crate) fn settings_group(&self, title: &str, rows: Vec<AnyElement>) -> impl IntoElement {
        widgets::settings_group(&self.palette, title, rows)
    }

    /// Like [`Self::settings_group`], but the group stretches to fill the height
    /// left over by its parent. Used by the version tab, where the release-notes
    /// area is the only part that scrolls — so the page itself never scrolls and
    /// there is just one scrollbar.
    pub(crate) fn settings_group_fill(
        &self,
        title: &str,
        rows: Vec<AnyElement>,
    ) -> impl IntoElement {
        widgets::settings_group_fill(&self.palette, title, rows)
    }

    /// The bare shell every settings row shares: standard row padding and
    /// minimum height, no card chrome (the enclosing group provides it). Use it
    /// directly when a row's control is bespoke, so it still lines up with the
    /// rows built from the `setting_row_*` helpers.
    pub(crate) fn row_shell(&self) -> Div {
        widgets::row_shell()
    }

    /// The label + description column of a settings row.
    ///
    /// The label stays on one line (ellipsised); the description wraps and the
    /// row grows, so text never slides under the control on the right.
    pub(crate) fn row_text(&self, label: &str, desc: &str) -> Div {
        widgets::row_text(&self.palette, label, desc)
    }

    /// A tappable row that opens something else (a list, a picker, a dialog):
    /// label + description on the left, a chevron on the right. Used by the
    /// blacklist / shortcut / latest-hotkey entries so they share one shape.
    pub(crate) fn setting_row_opener(
        &self,
        label: &str,
        desc: &str,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let p = self.palette;
        let on_click = Rc::new(on_click);
        self.row_shell()
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.bg(p.hover))
            .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(self.row_text(label, desc))
            .child(widgets::icon(
                "ChevronDownIcon",
                IconSize::Sm.scaled(),
                p.ink_2,
            ))
    }

    /// Render a settings row with an animated toggle switch on the right.
    ///
    /// Single-control row: kept on one line, but the description wraps and the
    /// card grows instead of the text ever sliding under the toggle.
    fn setting_row_with_toggle(
        &self,
        label: &str,
        desc: &str,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let p = self.palette;
        // 开关的滑动动画由 gpui-kit 的 motion 持有，id 用行标题保证唯一。
        let control = widgets::toggle(&p, label, label, value, false, on_toggle, window, cx);
        widgets::settings_row(&p, label, desc, control)
    }

    /// Render a toggle row with common boilerplate handled automatically.
    ///
    /// Handles entity cloning, dynamic description text, and `cx.notify()`.
    /// The `on_changed` closure receives references to the cloned state and
    /// settings-panel entities plus `&mut Window` and `&mut App`.
    #[allow(clippy::too_many_arguments)]
    fn render_toggle_row(
        &mut self,
        label: I18nKey,
        desc_on: I18nKey,
        desc_off: I18nKey,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_changed: impl Fn(&Entity<AppState>, &Entity<SettingsPanel>, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let state = self.state.clone();
        let this = cx.entity().clone();
        let desc = if value {
            desc_on.text()
        } else {
            desc_off.text()
        };
        self.setting_row_with_toggle(label.text(), desc, value, window, cx, move |window, app| {
            on_changed(&state, &this, window, app);
            this.update(app, |_panel, cx| cx.notify());
        })
    }

    /// Render a settings card for option groups (≥2 buttons).
    ///
    /// Two-row layout mirroring the copy-sound / sync-backend cards: the
    /// header holds the label and a description that may wrap to any number of
    /// lines, a divider separates it from the footer, and the option buttons
    /// fill the footer row with equal widths. Because the buttons never share a
    /// line with the text, larger font sizes can't overlap the description.
    /// Two-line settings card (option groups, sliders): the header holds the
    /// label and a description that may wrap to any number of lines, a divider
    /// separates it from the footer, and the footer never shares a line with the
    /// text — so larger font sizes can't overlap the control.
    fn row_header(&self, label: &str, desc: &str) -> Div {
        div()
            .px(Space::Lg.scaled())
            .pt(Space::Lg.scaled())
            .pb(Space::Md.scaled())
            .child(widgets::row_text(&self.palette, label, desc))
    }

    fn setting_row_with_options(
        &self,
        label: &str,
        desc: &str,
        options: &[(&'static str, &'static str)],
        active_key: &str,
        on_select: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let p = self.palette;
        let on_select = Rc::new(on_select);

        div()
            .flex()
            .flex_col()
            .child(self.row_header(label, desc))
            .child(div().h(px(1.)).bg(p.stroke))
            // --- Footer: equal-width option buttons on their own row ---
            .child(
                div()
                    .px(Space::Sm.scaled())
                    .py(Space::Sm.scaled())
                    .flex()
                    .flex_row()
                    .gap(Space::Sm.scaled())
                    .children(options.iter().map(|(key, display_label)| {
                        let selected = *key == active_key;
                        let tone = if selected {
                            widgets::Tone::Accent
                        } else {
                            widgets::Tone::Plain
                        };
                        let key = *key;
                        let on_select = on_select.clone();

                        widgets::btn(
                            &p,
                            ElementId::Name(format!("{label}:opt:{key}").into()),
                            *display_label,
                            None,
                            tone,
                        )
                        .flex_1()
                        .justify_center()
                        .on_click(move |_, window, cx| on_select(key, window, cx))
                    })),
            )
    }

    /// The layered palette every slider in the settings shares: track < fill <
    /// ticks < knob, derived from the theme accent so it adapts to light/dark.
    pub(crate) fn slider_colors(&self) -> SteppedSliderColors {
        SteppedSliderColors::from_palette(&self.palette)
    }

    /// Caller-owned drag state for the slider with `id`, created on first use.
    pub(crate) fn slider_drag(&mut self, id: &'static str, initial: usize) -> SliderDragState {
        self.slider_drags
            .entry(id)
            .or_insert_with(|| SliderDragState::new(initial))
            .clone()
    }

    /// Two-row settings card whose footer hosts a stepped slider (used by the
    /// font-size entry). Mirrors `setting_row_with_options` layout: wrapping
    /// header text, a divider, then the slider on its own full-width row.
    #[allow(clippy::too_many_arguments)]
    fn setting_row_with_slider(
        &mut self,
        label: &str,
        desc: &str,
        detents: Vec<SliderDetent>,
        active: usize,
        slider_id: &'static str,
        on_change: impl Fn(usize, &mut Window, &mut App) + 'static,
        on_preview: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let drag = self.slider_drag(slider_id, active);
        let p = self.palette;
        let colors = self.slider_colors();

        div()
            .flex()
            .flex_col()
            .child(self.row_header(label, desc))
            .child(div().h(px(1.)).bg(p.stroke))
            .child(
                div()
                    .px(Space::Xl.scaled())
                    .pt(Space::Md.scaled())
                    .pb(Space::Sm.scaled())
                    .child(
                        SteppedSlider::new(slider_id, detents)
                            .value(active)
                            .label_inset(18.)
                            .drag_state(drag)
                            .colors(colors)
                            .on_change(on_change)
                            .on_preview(on_preview),
                    ),
            )
    }

    /// Render a settings row whose right side is a single value button that
    /// opens a picker (used by the custom-font entry).
    fn setting_row_with_value(
        &self,
        label: &str,
        desc: &str,
        value: String,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let p = self.palette;
        let on_click = Rc::new(on_click);

        widgets::row_shell()
            .child(widgets::row_text(&p, label, desc))
            // --- Right: current value + caret (never compresses, never overlaps) ---
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .flex_row()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .h(Ctrl::Small.scaled())
                    .px(Space::Md.scaled())
                    .rounded(Radius::Md.scaled())
                    .border(px(1.))
                    .border_color(p.stroke)
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                        cx.stop_propagation();
                        on_click(window, cx);
                    })
                    .child(
                        div()
                            .max_w(px(150.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(typography::font(type_role::BODY))
                            .text_color(p.ink_2)
                            .child(value),
                    )
                    .child(widgets::icon(
                        "ChevronDownIcon",
                        IconSize::Sm.scaled(),
                        p.ink_3,
                    )),
            )
    }

    /// Open the custom-font picker, enumerating installed families on first use
    /// so the system font list is scanned at most once per panel lifetime.
    pub fn open_font_picker(&mut self, cx: &mut Context<Self>) {
        if self.available_fonts.is_none() {
            self.available_fonts = Some(Rc::new(crate::ui::font::available_families(cx)));
        }
        self.close_app_list_popups();
        self.latest_hotkeys_popup_open = false;
        self.config_sync_menu_open = false;
        self.font_picker_open = true;
        self.font_picker_scroll_frames = 4;
        cx.notify();
    }

    /// Close the custom-font picker (state only — the caller must notify).
    pub fn close_font_picker(&mut self, cx: &mut Context<Self>) {
        self.font_picker_open = false;
        cx.notify();
    }

    /// Apply a chosen font family and notify the window to re-render.
    fn apply_font_family(&mut self, family: String, cx: &mut Context<Self>) {
        let state = self.state.clone();
        state.update(cx, |s, _cx| {
            s.settings.font_family = family.clone();
            s.settings.save();
        });
        crate::ui::font::set_family(&family);
        crate::ui::font::apply_to_global_theme(cx);
        self.font_picker_open = false;
        cx.emit(SettingsEvent::FontChanged);
        cx.notify();
    }

    /// Absolute overlay covering the settings panel: a dim backdrop with
    /// a centered, size-capped card holding a scrollable (with scrollbar) list
    /// of font families, each previewed in its own typeface.
    pub(crate) fn render_font_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.font_picker_open {
            return div().into_any_element();
        }
        let p = self.palette;
        let hover_bg = Rgba::from(p.hover);
        let fonts = self.available_fonts.clone().unwrap_or_default();
        let current = crate::ui::font::family();
        let system_label = I18nKey::FontFamilySystem.text();
        let sample = I18nKey::FontFamilySample.text();
        let this = cx.entity().clone();

        // Persistent scroll handle for the family list. On open we align the
        // row matching the active font with the top of the list, so the current
        // choice is immediately visible instead of starting at the top of a
        // long list.
        let scroll_handle = window
            .use_keyed_state(
                ElementId::Name("font-picker-scroll-handle".into()),
                cx,
                |_, _| ScrollHandle::default(),
            )
            .read(cx)
            .clone();
        if self.font_picker_scroll_frames > 0 {
            self.font_picker_scroll_frames -= 1;
            let current_index = if current.is_empty() {
                0
            } else {
                fonts
                    .iter()
                    .position(|f| *f == current)
                    .map_or(0, |i| i + 1)
            };
            // Compute the offset from the *measured* row bounds instead of
            // asking the handle to scroll and hoping the request lands: the row
            // position only exists after layout, and an early offset is what
            // left the target off screen.
            if let Some(row) = scroll_handle.bounds_for_item(current_index) {
                let viewport_top = f32::from(scroll_handle.bounds().top());
                let row_top = f32::from(row.top());
                let max_scroll = f32::from(scroll_handle.max_offset().y);
                let y = px((viewport_top - row_top).clamp(-max_scroll, 0.0));
                scroll_handle.set_offset(point(px(0.), y));
            }
            // Repaint until the alignment has been applied (the row bounds may
            // not exist on the very first frame).
            let this = this.clone();
            window.defer(cx, move |_window, cx| {
                this.update(cx, |_panel, cx| cx.notify());
            });
        }

        let row_bg_transparent = p.background.opacity(0.);
        let row_text_selected = p.on_accent;
        let mk_entry =
            |family: String, display: String, preview_family: SharedString| -> AnyElement {
                let selected = family == current;
                let id = if family.is_empty() {
                    "font-family-system".to_string()
                } else {
                    format!("font-family-{}", family)
                };
                div()
                    .id(SharedString::from(id))
                    .h(px(34.))
                    .flex_shrink_0()
                    .px(Space::Md.scaled())
                    .mx(Space::Xs.scaled())
                    .rounded(Radius::Md.scaled())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .bg(if selected {
                        p.accent
                    } else {
                        row_bg_transparent
                    })
                    .cursor(CursorStyle::PointingHand)
                    .when(!selected, |d| d.hover(move |s| s.bg(hover_bg)))
                    .on_mouse_down(MouseButton::Left, {
                        let this = this.clone();
                        move |_ev, _window, cx| {
                            let family = family.clone();
                            this.update(cx, |panel, cx| panel.apply_font_family(family, cx));
                        }
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(preview_family.clone())
                            .text_size(typography::font(type_role::BASE))
                            .text_color(if selected { row_text_selected } else { p.ink_2 })
                            .child(display),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .ml(Space::Md.scaled())
                            .font_family(preview_family)
                            .text_size(typography::font(type_role::CAPTION))
                            .text_color(if selected { row_text_selected } else { p.ink_3 })
                            .child(sample),
                    )
                    .into_any_element()
            };

        let entries: Vec<AnyElement> = std::iter::once(mk_entry(
            String::new(),
            system_label.to_string(),
            crate::ui::font::system_family(),
        ))
        .chain(
            fonts
                .iter()
                .map(|name| mk_entry(name.clone(), name.clone(), SharedString::from(name.clone()))),
        )
        .collect();

        crate::ui::components::modal_overlay::modal_overlay()
            .p(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, {
                let this = this.clone();
                move |_ev, _window, cx| {
                    this.update(cx, |panel, cx| panel.close_font_picker(cx));
                }
            })
            .child(
                widgets::dialog_panel(&p)
                    .w_full()
                    .max_w(px(320.))
                    .h_full()
                    .max_h(px(460.))
                    .cursor(CursorStyle::Arrow)
                    .on_mouse_down(MouseButton::Left, |_ev, _window, cx| cx.stop_propagation())
                    // --- Header ---
                    .child(
                        div()
                            .h(px(44.))
                            .flex_shrink_0()
                            .px(px(12.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(typography::font(type_role::BASE))
                                    .font_weight(typography::LABEL_WEIGHT)
                                    .text_color(p.ink)
                                    .child(I18nKey::FontFamilyPicker.text()),
                            )
                            .child(
                                div()
                                    .w(Ctrl::Small.scaled())
                                    .h(Ctrl::Small.scaled())
                                    .rounded(Radius::Md.scaled())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor(CursorStyle::PointingHand)
                                    .on_mouse_down(MouseButton::Left, {
                                        let this = this.clone();
                                        move |_ev, _window, cx| {
                                            this.update(cx, |panel, cx| {
                                                panel.close_font_picker(cx)
                                            });
                                        }
                                    })
                                    .child(widgets::icon(
                                        "CloseIcon",
                                        IconSize::Sm.scaled(),
                                        p.ink_2,
                                    )),
                            ),
                    )
                    .child(div().h(px(1.)).flex_shrink_0().bg(p.stroke))
                    // --- Scrollable family list + vertical scrollbar ---
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(
                                div()
                                    .id("font-picker-scroll")
                                    .size_full()
                                    .py(px(4.))
                                    .pr(px(6.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll_handle)
                                    .children(entries),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .child(
                                        Scrollbar::new(&scroll_handle)
                                            .id("font-picker-scrollbar")
                                            .axis(ScrollbarAxis::Vertical),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Clear pending hotkey confirm dialog.
    pub fn clear_hotkey_confirm(&mut self, cx: &mut Context<Self>) {
        self.hotkey_confirm = None;
        cx.notify();
    }

    /// Clear pending paste shortcut recording state.
    pub fn clear_paste_shortcut_state(&mut self, cx: &mut Context<Self>) {
        self.recording_paste_shortcut = None;
        self.pending_paste_shortcut = None;
        cx.notify();
    }
}
