//! Changelog page — the bundled `RELEASES.md` entries behind the version tab.
//!
//! Rendered in place of the tab content (the tab bar is hidden while it is
//! open) so reading a release history is a page, not a cramped popup. Scrolling
//! reuses the settings scroll area; the page itself adds no nested scrollbars.

use crate::ui::font::fs;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::text::{TextView, TextViewStyle};

use crate::core::changelog;
use crate::core::i18n_keys::I18nKey;

use super::SettingsPanel;

impl SettingsPanel {
    /// Open the changelog page, scrolled to the newest version.
    pub fn open_changelog(&mut self, cx: &mut Context<Self>) {
        self.changelog_open = true;
        // The page shares the settings scroll handle, which may be deep inside
        // the version tab when the row was clicked.
        self.scroll_handle.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// Leave the changelog page, back to the version tab.
    pub fn close_changelog(&mut self, cx: &mut Context<Self>) {
        if self.changelog_open {
            self.changelog_open = false;
            self.scroll_handle.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    pub(crate) fn render_changelog(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let accent = theme.accent;
        let accent_soft = theme.accent_soft;
        let divider = theme.divider;
        let text_1 = theme.text_1;
        let text_2 = theme.text_2;
        let current_version = env!("CARGO_PKG_VERSION");

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

        let mut list = div().flex().flex_col().pt(px(2.));
        for (index, entry) in changelog::versions().iter().enumerate() {
            if index > 0 {
                list = list.child(div().h(px(1.)).mt(px(12.)).mb(px(12.)).bg(divider));
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
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
                    .child(
                        // The version string doubles as the element id: entries
                        // are unique and never reordered within a build.
                        div().text_size(fs(12.)).text_color(text_2).child(
                            TextView::markdown(entry.version, entry.body)
                                .style(body_style.clone())
                                .selectable(false),
                        ),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .px(px(6.))
            .pb(px(8.))
            .child(list)
    }
}
