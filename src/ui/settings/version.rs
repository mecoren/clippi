//! Version settings tab — current version, update check, download progress, release notes.

use crate::ui::font::fs;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::text::{TextView, TextViewStyle};

use crate::core::i18n_keys::I18nKey;
use crate::services::update::UpdatePhase;
use crate::ui::design::size::Radius;
use crate::ui::widgets;
use crate::ui::widgets::ButtonExt;

use super::SettingsPanel;

impl SettingsPanel {
    pub fn render_version_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let wm = self.window_manager.clone();
        let this = cx.entity().clone();

        let app = self.state.read(cx);
        let phase = app.update_phase.clone();
        let update_available = app.update_available.clone();
        let auto_check = app.settings.auto_check_updates;
        let current_version = env!("CARGO_PKG_VERSION");
        let divider = p.stroke;
        let accent = p.accent;
        let text_1 = p.ink;
        let text_2 = p.ink_2;
        let text_3 = p.ink_3;

        let (status_text, btn_label, btn_disabled, mut on_click) =
            button_state(&phase, &update_available, &wm);
        let show_release_notes = update_available.is_some()
            && phase != UpdatePhase::Idle
            && phase != UpdatePhase::Checking
            && phase != UpdatePhase::UpToDate;

        // Fill the tab so the page itself never scrolls: with the bottom bar in
        // place the content area is shorter, and an unbounded page plus the
        // notes' own scrollbar would give two scrollbars.
        let mut container = div().flex().flex_col().gap(px(12.)).pt(px(8.)).h_full();

        // --- Updates (release notes appear in the same card when present) ---
        let mut update_rows: Vec<AnyElement> = vec![div()
            .relative()
            .flex_shrink_0()
            .px(px(8.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            // Version number
            .child(
                div()
                    .text_size(fs(13.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_1)
                    // `Clippi (dev) v0.5.6` for a local build — the same marker
                    // the titlebar and tray use.
                    .child(format!(
                        "{} v{}",
                        crate::core::channel::display_name(),
                        current_version
                    )),
            )
            // Status text (may wrap, below version)
            .child(
                div()
                    .w_full()
                    .text_size(fs(10.))
                    .text_color(if matches!(phase, UpdatePhase::Error(_)) {
                        p.danger
                    } else {
                        accent
                    })
                    .child(status_text),
            )
            // Action row: auto-check checkbox + action button
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    // ── Left: compact auto-check checkbox ──
                    .child({
                        let this = this.clone();
                        let check_icon = if auto_check {
                            "CircleCheckIcon"
                        } else {
                            "CircleIcon"
                        };
                        let check_color = if auto_check { accent } else { text_3 };
                        div()
                            .relative()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .cursor(CursorStyle::PointingHand)
                            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                                this.update(cx, |panel, cx| {
                                    panel.state.update(cx, |s, _cx| {
                                        s.settings.auto_check_updates =
                                            !s.settings.auto_check_updates;
                                        s.settings.save();
                                    });
                                    cx.notify();
                                });
                            })
                            .child(widgets::icon_moving(
                                "version-auto-check",
                                check_icon,
                                fs(12.),
                                check_color,
                            ))
                            .child(widgets::icon_probe(
                                "version-auto-check",
                                CursorStyle::PointingHand,
                            ))
                            .child(
                                div()
                                    .text_size(fs(12.))
                                    .text_color(text_3)
                                    .child(I18nKey::SettingAutoCheckUpdate.text()),
                            )
                    })
                    // ── Right: action button ──
                    .child({
                        let handler = on_click.take();
                        widgets::btn_small(
                            &p,
                            "version-update-action",
                            btn_label,
                            None,
                            widgets::Tone::Accent,
                        )
                        .inert_dimmed(btn_disabled)
                        .when_some(handler, |button, handler| {
                            button.on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                                handler(ev, window, cx)
                            })
                        })
                    }),
            )
            // Progress bar (only during download)
            // GitHub link icon (absolute top-right, doesn't affect card height)
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .right(px(12.))
                    .group("gh-icon")
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(MouseButton::Left, |_ev, _window, _cx| {
                        // This build's own repository — `Cargo.toml`'s `repository`,
                        // the same value the update check queries.
                        if let Some(url) = crate::services::update::releases_page_url() {
                            std::thread::spawn(move || {
                                crate::services::update::open_releases_page(&url);
                            });
                        }
                    })
                    // Gray icon (default)
                    .child(widgets::icon("GithubIcon", fs(26.), text_3))
                    // Green icon (shown on group hover)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .opacity(0.0)
                            .group_hover("gh-icon", |s| s.opacity(1.0))
                            .child(widgets::icon_moving(
                                "version-github-lit",
                                "GithubIcon",
                                fs(26.),
                                accent,
                            )),
                    )
                    .child(widgets::icon_probe(
                        "version-github-lit",
                        CursorStyle::PointingHand,
                    )),
            )
            .when(matches!(phase, UpdatePhase::Downloading { .. }), |el| {
                el.child(render_progress_bar(
                    match phase {
                        UpdatePhase::Downloading { progress } => progress,
                        _ => 0,
                    },
                    accent.into(),
                    divider.into(),
                ))
            })
            .into_any_element()];
        if show_release_notes {
            let build_notes = |el: Div| {
                let notes = update_available
                    .as_ref()
                    .map(|i| i.release_notes.clone())
                    .unwrap_or_default();
                if notes.is_empty() {
                    return el;
                }
                el.flex_1().min_h(px(0.)).flex().flex_col().child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            // Title inside the box
                            div()
                                .px(px(8.))
                                .pt(px(8.))
                                .pb(px(4.))
                                .border_b(px(1.))
                                .border_color(divider)
                                .child(
                                    div()
                                        .text_size(fs(12.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(text_1)
                                        .child(I18nKey::VersionReleaseNotes.text()),
                                ),
                        )
                        .child({
                            let style = TextViewStyle::default()
                                .paragraph_gap(rems(0.2))
                                .heading_font_size(|level, base| {
                                    // Scale headings down to fit the compact settings UI.
                                    let sizes =
                                        [px(12.), px(11.5), px(11.), px(10.5), px(10.), px(10.)];
                                    sizes
                                        .get(level.saturating_sub(1) as usize)
                                        .copied()
                                        .unwrap_or(base)
                                });
                            div()
                                .flex_1()
                                .min_h(px(0.))
                                .px(px(8.))
                                .py(px(6.))
                                .overflow_y_scrollbar()
                                .text_size(fs(12.))
                                .text_color(text_2)
                                .child(
                                    TextView::markdown("version-release-notes", notes)
                                        .style(style)
                                        .selectable(false),
                                )
                        }),
                )
            };
            update_rows.push(build_notes(div()).into_any_element());
        }
        container = container.child(if show_release_notes {
            self.settings_group_fill(I18nKey::GroupUpdate.text(), update_rows)
                .into_any_element()
        } else {
            self.settings_group(I18nKey::GroupUpdate.text(), update_rows)
                .into_any_element()
        });

        // --- Changelog (bundled release notes, one page for every version) ---
        container = container.child(div().flex_shrink_0().child(self.settings_group(
            I18nKey::Changelog.text(),
            vec![self
                    .setting_row_opener(
                        I18nKey::Changelog.text(),
                        I18nKey::DescChangelog.text(),
                        move |_window, cx| {
                            this.update(cx, |panel, cx| panel.open_changelog(cx));
                        },
                    )
                    .into_any_element()],
        )));

        container
    }
}

type ButtonClickHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type ButtonState = (String, String, bool, Option<ButtonClickHandler>);

/// Determine the unified button state from the current update phase.
fn button_state(
    phase: &UpdatePhase,
    update_available: &Option<crate::services::update::UpdateInfo>,
    wm: &gpui::Entity<crate::ui::window_manager::WindowManager>,
) -> ButtonState {
    match phase {
        UpdatePhase::Idle | UpdatePhase::UpToDate => (
            format!("✓ {}", I18nKey::VersionUpToDate.text()),
            I18nKey::VersionCheckNow.text().to_string(),
            false,
            {
                let wm = wm.clone();
                Some(Box::new(move |_ev, _window, cx| {
                    wm.update(cx, |wm, cx| wm.start_update_check(cx));
                }))
            },
        ),
        UpdatePhase::Checking => (
            I18nKey::VersionChecking.text().to_string(),
            I18nKey::VersionChecking.text().to_string(),
            true,
            None,
        ),
        UpdatePhase::UpdateAvailable => {
            let latest = update_available
                .as_ref()
                .map(|i| i.latest_version.clone())
                .unwrap_or_default();
            (
                format!("● {} v{}", I18nKey::VersionFound.text(), latest),
                I18nKey::VersionDownload.text().to_string(),
                false,
                {
                    let wm = wm.clone();
                    Some(Box::new(move |_ev, _window, cx| {
                        wm.update(cx, |wm, cx| wm.start_update_download(cx));
                    }))
                },
            )
        }
        UpdatePhase::Downloading { progress } => (
            I18nKey::VersionDownloading
                .text()
                .replace("{0}", &progress.to_string()),
            format!("{}%", progress),
            true,
            None,
        ),
        UpdatePhase::Verifying => (
            I18nKey::VersionVerifying.text().to_string(),
            I18nKey::VersionVerifying.text().to_string(),
            true,
            None,
        ),
        UpdatePhase::Installing => (
            I18nKey::VersionInstalling.text().to_string(),
            I18nKey::VersionInstalling.text().to_string(),
            true,
            None,
        ),
        UpdatePhase::ReadyToRestart => (
            I18nKey::VersionReady.text().to_string(),
            I18nKey::BtnRestartNow.text().to_string(),
            false,
            {
                let wm = wm.clone();
                Some(Box::new(move |_ev, _window, cx| {
                    wm.update(cx, |wm, cx| wm.do_update_restart(cx));
                }))
            },
        ),
        UpdatePhase::Error(msg) => (
            msg.clone(),
            I18nKey::VersionCheckNow.text().to_string(),
            false,
            {
                let wm = wm.clone();
                Some(Box::new(move |_ev, _window, cx| {
                    wm.update(cx, |wm, cx| wm.start_update_check(cx));
                }))
            },
        ),
    }
}

/// Simple progress bar (colored fill inside a track).
fn render_progress_bar(progress: u8, fill_color: Rgba, track_color: Rgba) -> impl IntoElement {
    let pct = progress.min(100);
    div()
        .w_full()
        .h(px(4.))
        .rounded(Radius::Sm.scaled())
        .bg(track_color)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(pct as f32 / 100.0))
                .rounded(Radius::Sm.scaled())
                .bg(fill_color),
        )
}
