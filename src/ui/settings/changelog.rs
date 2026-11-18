//! Changelog page — the bundled `RELEASES.md` entries behind the version tab.
//!
//! The page is a list: one row per version with its release date, like the
//! clipboard history. Opening a row shows that version's notes in the shared
//! centered dialog instead of stacking every release on one long page.
//! Scrolling reuses the settings scroll area; the page adds no scrollbars.

use crate::ui::font::fs;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::text::{TextView, TextViewStyle};

use crate::core::changelog;
use crate::core::i18n_keys::I18nKey;
use crate::ui::design::size::Space;
use crate::ui::widgets;

use super::SettingsPanel;

impl SettingsPanel {
    /// Open the changelog version list, scrolled to the newest version.
    pub fn open_changelog(&mut self, cx: &mut Context<Self>) {
        self.changelog_open = true;
        self.changelog_detail = None;
        // The page shares the settings scroll handle, which may be deep inside
        // the version tab when the row was clicked.
        self.scroll_handle.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// Leave the changelog page, back to the version tab.
    pub fn close_changelog(&mut self, cx: &mut Context<Self>) {
        if self.changelog_open {
            self.changelog_open = false;
            self.changelog_detail = None;
            self.scroll_handle.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// Open one version's notes in the changelog dialog.
    pub fn open_changelog_entry(&mut self, version: &'static str, cx: &mut Context<Self>) {
        self.changelog_detail = Some(version);
        cx.notify();
    }

    /// Close the notes dialog, back to the version list.
    pub fn close_changelog_entry(&mut self, cx: &mut Context<Self>) {
        if self.changelog_detail.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn render_changelog(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let theme = &self.theme;
        let accent = theme.accent;
        let accent_soft = theme.accent_soft;
        let text_1 = theme.text_1;
        let text_3 = theme.text_3;
        let current_version = env!("CARGO_PKG_VERSION");
        let this = cx.entity().clone();

        let mut card = widgets::card(&p);
        for (index, entry) in changelog::versions().iter().enumerate() {
            if index > 0 {
                card = card.child(div().h(px(1.)).mx(Space::Lg.scaled()).bg(p.stroke));
            }
            let open = this.clone();
            let version = entry.version;
            card = card.child(
                self.row_shell()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(p.hover))
                    .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                        cx.stop_propagation();
                        open.update(cx, |panel, cx| panel.open_changelog_entry(version, cx));
                    })
                    // Version (+ current-version marker) on the left…
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(fs(13.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(text_1)
                                    .child(format!("v{}", entry.version)),
                            )
                            .when(entry.version == current_version, |row| {
                                row.child(
                                    div()
                                        .px(px(4.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(accent_soft)
                                        .text_size(fs(10.))
                                        .text_color(accent)
                                        .child(I18nKey::ChangelogCurrent.text()),
                                )
                            }),
                    )
                    // …release date on the right.
                    .when_some(entry.date, |row, date| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_size(fs(11.))
                                .text_color(text_3)
                                .child(date),
                        )
                    }),
            );
        }

        div().w_full().flex().flex_col().pt(px(2.)).child(card)
    }

    /// Notes of one version, laid out for the body of the changelog dialog.
    pub(crate) fn render_changelog_notes(&self, version: &str) -> AnyElement {
        let Some(entry) = changelog::version(version) else {
            return div().into_any_element();
        };
        let text_2 = self.theme.text_2;
        // `###` section headings are the only headings in a body; keep them just
        // above body weight so a version reads as one block.
        let body_style = TextViewStyle::default()
            .paragraph_gap(rems(0.16))
            .heading_font_size(|level, base| {
                let sizes = [px(12.), px(11.5), px(11.), px(10.5), px(10.), px(10.)];
                sizes
                    .get(level.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or(base)
            });

        div()
            .w_full()
            .px(px(2.))
            .text_size(fs(12.))
            .text_color(text_2)
            .child(
                // `TextView` caches its laid-out state per id, so the id has to
                // change with the version — the dialog is reused, the notes are not.
                TextView::markdown(
                    ElementId::Name(format!("changelog-notes:{}", entry.version).into()),
                    entry.body,
                )
                .style(body_style)
                .selectable(false),
            )
            .into_any_element()
    }
}
