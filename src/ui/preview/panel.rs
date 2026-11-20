//! Hover preview panel — content and interaction for the native flyout.
//!
//! The panel is rendered inside its own non-activating popup window (see
//! `platform::preview_flyout`), so the main window keeps focus and the list's
//! keyboard model stays intact. There is no in-window placement here: the
//! platform attachment docks the window beside the main panel.

use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::content::{self, PreviewKind, PreviewModel};
use crate::core::i18n_keys::I18nKey;
use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{IconSize, Radius, Space};
use crate::ui::design::typography::{self, type_role};
use crate::ui::rich_preview;
use crate::ui::transition::keyed_transition;
use crate::ui::widgets::{self, Tone};

/// The flyout lives in its own native window, so its size no longer depends on
/// the narrow main list.
const PANEL_WINDOW_WIDTH: f32 = 340.;
const PANEL_WINDOW_MAX_HEIGHT: f32 = 480.;
const PANEL_WINDOW_MIN_HEIGHT: f32 = 120.;
const HEADER_HEIGHT: f32 = 34.;
const FOOTER_HEIGHT: f32 = 38.;
const BODY_LINE_HEIGHT: f32 = 20.;
const BODY_PADDING: f32 = 16.;
const MAX_TEXT_LINES: usize = 12;
const MAX_TEXT_CHARS: usize = 900;
const MAX_HTML_LINES: usize = 10;
const MAX_FILE_ROWS: usize = 5;

/// Hover promotion is for content the compact sheet cannot show in full:
/// images and file lists always benefit from the large view, text only when
/// the excerpt dropped something or a line is visually clipped by width.
fn auto_expand_for(
    kind: PreviewKind,
    rich_shown: bool,
    html_clipped: bool,
    excerpt_truncated: bool,
    lines: &[String],
) -> bool {
    match kind {
        PreviewKind::Image | PreviewKind::Files => true,
        PreviewKind::Html if rich_shown => html_clipped,
        _ => excerpt_truncated || lines.iter().any(|line| line_overflows_body(line)),
    }
}

/// Conservative width estimate: would this line be clipped by `.truncate()`
/// at the compact body width? Uses per-char classification (CJK full-width,
/// ASCII ~0.6em) so both scripts are covered without font measurement.
fn line_overflows_body(line: &str) -> bool {
    let scale = crate::ui::font::scale();
    let font_size = type_role::BODY * scale;
    let body_width = PANEL_WINDOW_WIDTH - 2. * f32::from(Space::Lg.scaled());
    let mut width = 0.0f32;
    for ch in line.chars() {
        width += if ch > '\u{2E7F}' {
            font_size
        } else {
            font_size * 0.6
        };
        if width > body_width {
            return true;
        }
    }
    false
}

pub enum PreviewPanelEvent {
    /// Open the standalone preview window for this item.
    Expand(i64),
    /// Copy the item (handled by the list, which owns `AppState`).
    Copy(i64),
    /// Pointer entered/left the panel. The list uses this to keep the panel
    /// open while the pointer travels between card and flyout.
    HoverChanged(bool),
}

impl EventEmitter<PreviewPanelEvent> for PreviewPanelView {}

pub struct PreviewPanelView {
    model: Option<PreviewModel>,
    html_lines: Option<Vec<Vec<rich_preview::StyledHtmlSpan>>>,
    /// Cached excerpt computed once per show; render and sizing reuse it
    /// instead of re-scanning the full text every animation frame.
    excerpt_lines: Vec<String>,
    excerpt_truncated: bool,
    /// The native window's screen rect, refreshed every render. Used by the
    /// list's cursor watchdog because a non-activating popup does not
    /// reliably deliver hover-out when the pointer leaves it.
    screen_bounds: Bounds<Pixels>,
    image_cache: Entity<RetainAllImageCache>,
    visible: bool,
    hovered: bool,
    copied: bool,
    /// Whether hovering the sheet may promote it to the full preview: text
    /// that already fits the compact sheet stays small.
    auto_expand: bool,
    _copy_task: Option<Task<()>>,
    generation: u64,
}

impl PreviewPanelView {
    pub fn new(cx: &mut App) -> Self {
        Self {
            model: None,
            html_lines: None,
            excerpt_lines: Vec::new(),
            excerpt_truncated: false,
            screen_bounds: Bounds::default(),
            image_cache: RetainAllImageCache::new(cx),
            visible: false,
            hovered: false,
            copied: false,
            auto_expand: true,
            _copy_task: None,
            generation: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Hover dwell may grow the sheet only when the content does not fit it;
    /// the manual "放大查看" button is never gated by this.
    pub fn auto_expand_allowed(&self) -> bool {
        self.auto_expand
    }

    pub fn item_id(&self) -> Option<i64> {
        self.model.as_ref().map(|model| model.id)
    }

    pub fn screen_bounds(&self) -> Bounds<Pixels> {
        self.screen_bounds
    }

    /// Native window size for the content currently shown, without rebuilding
    /// the model (so a collapse onto the same item does not restart the fade).
    pub fn current_size(&self) -> Option<Size<Pixels>> {
        self.model
            .as_ref()
            .map(|m| panel_size(m, self.excerpt_lines.len()))
    }

    /// The wrapper keeps this fresh while the expanded full view is showing,
    /// so the list's cursor watchdog tracks the enlarged window too.
    pub fn set_screen_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.screen_bounds = bounds;
    }

    /// Show (or retarget) the flyout in its own window. The window size is
    /// returned so the caller can position/resize the native window.
    pub fn show_native(
        &mut self,
        item: &crate::core::types::ClipboardItem,
        privacy_types: &[String],
        cx: &mut Context<Self>,
    ) -> Size<Pixels> {
        let model = PreviewModel::from_item(item, privacy_types);
        // Compute the excerpt once; render, sizing and promotion all reuse it.
        let (lines, truncated) = content::excerpt(&model.text, MAX_TEXT_LINES, MAX_TEXT_CHARS);
        let size = panel_size(&model, lines.len());
        // The sheet shows at most MAX_HTML_LINES rows; parsing stops there
        // instead of walking a whole large document on every hover.
        let html_capped = if model.kind == PreviewKind::Html {
            model
                .html
                .as_deref()
                .and_then(|html| rich_preview::parse_styled_html_lines_capped(html, MAX_HTML_LINES))
        } else {
            None
        };
        let html_clipped = html_capped.as_ref().is_some_and(|(_, clipped)| *clipped);
        self.html_lines = html_capped.map(|(lines, _)| lines);
        let rich_shown = self
            .html_lines
            .as_ref()
            .is_some_and(|lines| !lines.is_empty());
        self.auto_expand = auto_expand_for(model.kind, rich_shown, html_clipped, truncated, &lines);
        self.excerpt_lines = lines;
        self.excerpt_truncated = truncated;
        self.model = Some(model);
        self.visible = true;
        self.hovered = false;
        self.copied = false;
        self._copy_task = None;
        self.generation = self.generation.wrapping_add(1);
        cx.notify();
        size
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.hovered = false;
        self.model = None;
        self.html_lines = None;
        self.auto_expand = true;
        self.copied = false;
        self._copy_task = None;
        // Thumbnails are small, but the panel can stay hidden for hours; let
        // the decoded texture go with the rest of the preview state.
        self.image_cache = RetainAllImageCache::new(cx);
        cx.notify();
    }

    fn render_body(&self, p: &Palette) -> AnyElement {
        let Some(model) = self.model.as_ref() else {
            return div().into_any_element();
        };

        match model.kind {
            PreviewKind::Image => {
                if !model.image_exists {
                    return div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(typography::font(type_role::CAPTION))
                        .text_color(p.ink_3)
                        .child(I18nKey::PreviewImageMissing.text())
                        .into_any_element();
                }
                let path = crate::platform::clipboard::image_thumbnail_path(model.content_hash);
                if path.is_none() {
                    crate::platform::clipboard::ensure_thumbnail_for_image(
                        &model.image_path,
                        model.content_hash,
                    );
                }
                match path {
                    Some(path) => div()
                        .size_full()
                        .child(
                            img(path)
                                .w_full()
                                .h_full()
                                .object_fit(ObjectFit::Contain)
                                .image_cache(&self.image_cache),
                        )
                        .into_any_element(),
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(typography::font(type_role::CAPTION))
                        .text_color(p.ink_3)
                        .child(I18nKey::PreviewLoading.text())
                        .into_any_element(),
                }
            }
            PreviewKind::Files => {
                if model.files.is_empty() {
                    return div()
                        .text_size(typography::font(type_role::BODY))
                        .text_color(p.ink_3)
                        .child(I18nKey::PreviewEmpty.text())
                        .into_any_element();
                }
                let rows = model
                    .files
                    .iter()
                    .take(MAX_FILE_ROWS)
                    .map(|file| {
                        div()
                            .h(px(26.))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(Space::Sm.scaled())
                            .child(widgets::icon(
                                if file.is_dir { "PathIcon" } else { "FilesIcon" },
                                IconSize::Sm.scaled(),
                                p.ink_3,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(typography::font(type_role::BODY))
                                    .text_color(p.ink)
                                    .child(file.name.clone()),
                            )
                    })
                    .collect::<Vec<_>>();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows)
                    .into_any_element()
            }
            PreviewKind::Html => {
                if let Some(lines) = self.html_lines.as_ref().filter(|lines| !lines.is_empty()) {
                    div()
                        .size_full()
                        .overflow_hidden()
                        .child(rich_preview::render_styled_html_lines(
                            lines.clone(),
                            p.ink.into(),
                            p.flyout.into(),
                        ))
                        .into_any_element()
                } else {
                    self.render_text_excerpt(model, p)
                }
            }
            PreviewKind::Text | PreviewKind::Markdown | PreviewKind::Rtf => {
                self.render_text_excerpt(model, p)
            }
        }
    }

    fn render_text_excerpt(&self, model: &PreviewModel, p: &Palette) -> AnyElement {
        if model.text.trim().is_empty() {
            return div()
                .text_size(typography::font(type_role::BODY))
                .text_color(p.ink_3)
                .child(I18nKey::PreviewEmpty.text())
                .into_any_element();
        }

        let mut column = div().flex().flex_col().gap(px(1.));
        for line in &self.excerpt_lines {
            if line.is_empty() {
                column = column.child(div().h(px(BODY_LINE_HEIGHT * 0.6)));
            } else {
                column = column.child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(typography::font(type_role::BODY))
                        .line_height(px(BODY_LINE_HEIGHT))
                        .text_color(p.ink)
                        .child(line.clone()),
                );
            }
        }
        if self.excerpt_truncated {
            column = column.child(
                div()
                    .text_size(typography::font(type_role::CAPTION))
                    .text_color(p.ink_3)
                    .child("…"),
            );
        }
        column.into_any_element()
    }
}

impl Render for PreviewPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        let Some(model) = self.model.as_ref() else {
            return div().into_any_element();
        };
        let p = Palette::current(cx);
        let opacity = keyed_transition(
            window,
            cx,
            ("preview-panel", self.generation),
            motion::fast(),
            0.0,
            motion::ease_out,
            1.0,
        );
        let id = model.id;
        let badge = model.badge;
        let meta = content::meta_text(model);

        let this = cx.entity();
        let copy_this = this.clone();
        let expand_this = this;

        // The native canvas is a transparent, frameless work-area window: the
        // card draws its own rounded edge, stroke and shadow so every visual
        // moves with the rendered content instead of a native window rect.
        div()
            .id(("preview-panel", self.generation))
            .size_full()
            .occlude()
            .bg(p.flyout)
            .rounded(Radius::Lg.scaled())
            .border(px(1.))
            .border_color(p.stroke)
            .when(cfg!(target_os = "windows"), |panel| {
                panel.shadow(widgets::flyout_shadow(p.dark))
            })
            .opacity(opacity)
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, hovered, _window, cx| {
                if this.hovered != *hovered {
                    this.hovered = *hovered;
                    cx.emit(PreviewPanelEvent::HoverChanged(*hovered));
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(HEADER_HEIGHT))
                    .flex_shrink_0()
                    .px(Space::Lg.scaled())
                    .flex()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .child(
                        div()
                            .px(px(5.))
                            .rounded(Radius::Sm.scaled())
                            .bg(p.tag_bg)
                            .text_size(typography::font(type_role::CAPTION))
                            .text_color(p.tag_ink)
                            .child(badge),
                    )
                    .child(
                        div()
                            .text_size(typography::font(type_role::BODY))
                            .font_weight(typography::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(I18nKey::PreviewTitle.text()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .truncate()
                            .text_size(typography::font(type_role::CAPTION))
                            .text_color(p.ink_3)
                            .child(meta),
                    ),
            )
            .child(div().h(px(1.)).flex_shrink_0().bg(p.stroke))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .px(Space::Lg.scaled())
                    .py(Space::Md.scaled())
                    .overflow_hidden()
                    .child(self.render_body(&p)),
            )
            .child(
                div()
                    .h(px(FOOTER_HEIGHT))
                    .flex_shrink_0()
                    .px(Space::Lg.scaled())
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(Space::Sm.scaled())
                    .child(
                        widgets::btn_small(
                            &p,
                            ElementId::Name(format!("preview-panel-copy-{id}").into()),
                            if self.copied {
                                I18nKey::PreviewCopied.text()
                            } else {
                                I18nKey::PreviewCopy.text()
                            },
                            Some(if self.copied { "CheckIcon" } else { "CopyIcon" }),
                            Tone::Plain,
                        )
                        .on_click(move |_, _, cx| {
                            copy_this.update(cx, |panel, cx| {
                                if let Some(id) = panel.item_id() {
                                    cx.emit(PreviewPanelEvent::Copy(id));
                                    panel.copied = true;
                                    panel._copy_task = Some(cx.spawn(async move |this, cx| {
                                        cx.background_executor()
                                            .timer(Duration::from_millis(1000))
                                            .await;
                                        let _ = this.update(cx, |panel, cx| {
                                            panel.copied = false;
                                            cx.notify();
                                        });
                                    }));
                                    cx.notify();
                                }
                            });
                        }),
                    )
                    .child(
                        widgets::btn_small(
                            &p,
                            ElementId::Name(format!("preview-panel-expand-{id}").into()),
                            I18nKey::PreviewExpand.text(),
                            Some("PreviewIcon"),
                            Tone::Accent,
                        )
                        .on_click(move |_, _, cx| {
                            expand_this.update(cx, |panel, cx| {
                                if let Some(id) = panel.item_id() {
                                    cx.emit(PreviewPanelEvent::Expand(id));
                                }
                            });
                        }),
                    ),
            )
            .into_any_element()
    }
}

/// Compute the native flyout size for one model. The window is independent of
/// the main list width and can grow taller than the old in-window sheet.
pub(crate) fn panel_size(model: &PreviewModel, excerpt_line_count: usize) -> Size<Pixels> {
    let width = PANEL_WINDOW_WIDTH;

    let body = match model.kind {
        PreviewKind::Text | PreviewKind::Markdown | PreviewKind::Rtf => {
            excerpt_line_count.max(1) as f32 * BODY_LINE_HEIGHT + BODY_PADDING
        }
        PreviewKind::Html => MAX_HTML_LINES as f32 * BODY_LINE_HEIGHT + BODY_PADDING,
        PreviewKind::Image => {
            let aspect = if model.has_known_image_size() {
                model.image_height as f32 / model.image_width as f32
            } else {
                0.6
            };
            (width * aspect).clamp(140., 320.) + BODY_PADDING * 0.5
        }
        PreviewKind::Files => {
            model.files.len().clamp(1, MAX_FILE_ROWS) as f32 * 28. + BODY_PADDING * 0.5
        }
    };
    let height = (HEADER_HEIGHT + body + FOOTER_HEIGHT)
        .clamp(PANEL_WINDOW_MIN_HEIGHT, PANEL_WINDOW_MAX_HEIGHT);
    size(px(width), px(height))
}

#[cfg(test)]
mod tests {
    use super::{
        auto_expand_for, line_overflows_body, panel_size, PreviewKind, PreviewModel,
        MAX_TEXT_CHARS, MAX_TEXT_LINES, PANEL_WINDOW_MAX_HEIGHT, PANEL_WINDOW_WIDTH,
    };
    use crate::core::types::{ClipboardItem, ContentType};
    use gpui::px;

    fn model(kind: PreviewKind) -> PreviewModel {
        let item = ClipboardItem::new_text(1, "hello", ContentType::PlainText, None, None);
        let mut model = PreviewModel::from_item(&item, &[]);
        model.kind = kind;
        model
    }

    #[test]
    fn window_width_is_fixed_and_independent_of_the_main_list() {
        for kind in [
            PreviewKind::Text,
            PreviewKind::Html,
            PreviewKind::Image,
            PreviewKind::Files,
        ] {
            let size = panel_size(&model(kind), 1);
            assert_eq!(size.width, px(PANEL_WINDOW_WIDTH));
            assert!(size.height <= px(PANEL_WINDOW_MAX_HEIGHT));
            assert!(size.height > px(0.));
        }
    }

    #[test]
    fn tall_image_is_roomier_but_still_capped() {
        let mut model = model(PreviewKind::Image);
        model.image_width = 100;
        model.image_height = 1000;
        let size = panel_size(&model, 1);
        assert_eq!(size.width, px(PANEL_WINDOW_WIDTH));
        assert!(size.height > px(360.));
        assert!(size.height <= px(PANEL_WINDOW_MAX_HEIGHT));
    }

    #[test]
    fn text_excerpt_uses_more_lines_than_the_old_sheet() {
        let mut model = model(PreviewKind::Text);
        model.text = "line\n".repeat(40);
        let (lines, _) = super::content::excerpt(&model.text, MAX_TEXT_LINES, MAX_TEXT_CHARS);
        let size = panel_size(&model, lines.len());
        assert!(size.height > px(300.));
        assert!(size.height <= px(360.));
    }

    #[test]
    fn hover_promotion_only_for_content_the_sheet_cannot_fit() {
        let short = vec!["1111111111".to_string()];
        assert!(!auto_expand_for(
            PreviewKind::Text,
            false,
            false,
            false,
            &short
        ));
        let multi = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        assert!(!auto_expand_for(
            PreviewKind::Markdown,
            false,
            false,
            false,
            &multi
        ));
        // More lines than the sheet shows: promotion allowed.
        assert!(auto_expand_for(
            PreviewKind::Text,
            false,
            false,
            true,
            &multi
        ));
        // A single line longer than the char cap also overflows.
        assert!(auto_expand_for(
            PreviewKind::Rtf,
            false,
            false,
            true,
            &short
        ));
        // Rich HTML: the line cap decides, not the plain projection.
        assert!(!auto_expand_for(
            PreviewKind::Html,
            true,
            false,
            false,
            &short
        ));
        assert!(auto_expand_for(
            PreviewKind::Html,
            true,
            true,
            false,
            &short
        ));
        // Degraded HTML falls back to the plain excerpt rule.
        assert!(!auto_expand_for(
            PreviewKind::Html,
            false,
            false,
            false,
            &short
        ));
        // Non-text kinds always promote.
        assert!(auto_expand_for(
            PreviewKind::Image,
            false,
            false,
            false,
            &[]
        ));
        assert!(auto_expand_for(
            PreviewKind::Files,
            false,
            false,
            false,
            &[]
        ));
    }

    #[test]
    fn width_clipped_single_line_triggers_promotion() {
        // A 234-char JSON line fits the char cap but overflows the body width.
        let json_line = r#"[{"name":"REQUEST:VALIDATION_ERROR","message":"Request validation failed","status":422,"log_id":4454851084,"fields":{"shipment.customs.items.0.code":["The shipment customs items 0 code field must be present."]}}]"#;
        let lines = vec![json_line.to_string()];
        assert!(line_overflows_body(json_line));
        assert!(auto_expand_for(
            PreviewKind::Text,
            false,
            false,
            false,
            &lines
        ));
        // A short line that fits the body width does not promote.
        let short_line = "hello world";
        assert!(!line_overflows_body(short_line));
        assert!(!auto_expand_for(
            PreviewKind::Text,
            false,
            false,
            false,
            &[short_line.to_string()]
        ));
    }

    #[test]
    fn cjk_line_overflow_detected_at_fewer_chars() {
        // CJK chars are full-width: ~26 chars overflow the 316px body at 12px font.
        let cjk = "中".repeat(30);
        assert!(line_overflows_body(&cjk));
        let short_cjk = "中".repeat(10);
        assert!(!line_overflows_body(&short_cjk));
    }
}
