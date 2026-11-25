//! Shared, read-only preview model.
//!
//! One classification pass turns a full [`ClipboardItem`] (loaded from the
//! database, never the trimmed list copy) into a [`PreviewModel`]. Both the
//! hover panel and the standalone preview window render that model, so the
//! two surfaces can never disagree about which representation to show.
//!
//! Everything in this file is UI-independent except the small helpers that
//! produce strings for `TextView::html`/`TextView::markdown`. Layout, colors
//! and interaction stay in `panel.rs` / `window.rs`.

use crate::core::html_text;
use crate::core::i18n_keys::I18nKey;
use crate::core::rtf_text;
use crate::core::secret::sensitive_preview_to_text_with_types;
use crate::core::types::{
    is_image_extension, ClipboardItem, DisplayKind, FileData, FileInfo, RichData,
};

/// EcoPaste degrades rendered HTML above 256 KB to plain text. Keep the same
/// contract: the boundary itself is allowed, only strictly larger documents
/// degrade.
pub const HTML_DEGRADE_BYTES: usize = 256 * 1024;

/// The full preview window lists at most this many file entries. The total
/// count is still reported in the header/footer.
pub const MAX_PREVIEW_FILES: usize = 64;

/// How the preview body is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Text,
    Markdown,
    Html,
    Rtf,
    Image,
    Files,
}

/// A preview-ready snapshot of one clipboard entry.
#[derive(Debug, Clone)]
pub struct PreviewModel {
    pub id: i64,
    /// Stable content hash: the thumbnail cache key and the React-style
    /// `key` that keeps `TextView` state from leaking across entries.
    pub content_hash: u64,
    pub kind: PreviewKind,
    /// Localized type badge shown in both surfaces (`MD`, `HTML`, `图片`, …).
    pub badge: &'static str,
    /// The plain-text projection. For HTML it is the OS plain-text payload and
    /// is what the degraded/fallback view renders; for RTF it is the decoded
    /// text. For Markdown it is the raw source (the "原文" tab).
    pub text: String,
    /// Raw HTML for the rich view. `None` when the entry is not HTML or the
    /// document exceeded [`HTML_DEGRADE_BYTES`].
    pub html: Option<String>,
    pub html_degraded: bool,
    pub image_path: String,
    /// Checked once when the model is built (image entries only); rendering
    /// must not stat the source file every frame.
    pub image_exists: bool,
    pub image_width: u32,
    pub image_height: u32,
    /// File entries, already truncated to [`MAX_PREVIEW_FILES`].
    pub files: Vec<FileInfo>,
    pub total_files: usize,
    /// A single, non-directory file with an image extension. The window checks
    /// that it exists and decodes before switching to the image branch.
    pub single_image_path: Option<String>,
    /// Character count of the plain-text projection.
    pub chars: usize,
    /// Original byte size (files/images) or the stored size for text.
    pub size: i64,
}

impl PreviewModel {
    /// Build the model from a full item (`Database::get_by_id`), not the
    /// trimmed list row. `privacy_types` applies the same masking the cards
    /// use, so a hover preview never reveals more than the list already does.
    pub fn from_item(item: &ClipboardItem, privacy_types: &[String]) -> Self {
        let rich = RichData::from_json(&item.rich_data);
        let display = item.display_kind();
        let badge = badge_label(display);

        let mut model = Self {
            id: item.id,
            content_hash: item.content_hash,
            kind: PreviewKind::Text,
            badge,
            text: item.full_text.clone(),
            html: None,
            html_degraded: false,
            image_path: item.image_path.clone(),
            image_exists: false,
            image_width: item.image_width,
            image_height: item.image_height,
            files: Vec::new(),
            total_files: 0,
            single_image_path: None,
            chars: char_count(&item.full_text),
            size: item.size,
        };

        match display {
            DisplayKind::Html => {
                let raw = rich.html.as_deref().filter(|html| !html.trim().is_empty());
                if let Some(raw) = raw {
                    if raw.len() > HTML_DEGRADE_BYTES {
                        model.html_degraded = true;
                        model.text = plain_text_fallback(item, &rich);
                    } else {
                        let normalized = html_text::normalize_clipboard_html(raw);
                        let visible = html_text::visible_text(&normalized);
                        if visible.trim().is_empty() {
                            // Metadata-only or unparseable fragment: keep the
                            // richer RTF/plain payload instead of an empty pane.
                            model.text = plain_text_fallback(item, &rich);
                        } else {
                            model.kind = PreviewKind::Html;
                            model.html = Some(raw.to_string());
                            model.text = visible;
                        }
                    }
                } else {
                    model.text = plain_text_fallback(item, &rich);
                }
                model.chars = model.text.chars().count();
            }
            DisplayKind::Markdown => {
                model.kind = PreviewKind::Markdown;
                model.text = item.full_text.clone();
                model.chars = model.text.chars().count();
            }
            DisplayKind::Rtf => {
                model.kind = PreviewKind::Rtf;
                model.text = rtf_plain_or_fallback(&rich, item);
                model.chars = model.text.chars().count();
            }
            DisplayKind::Image => {
                model.kind = PreviewKind::Image;
                model.image_exists =
                    !item.image_path.is_empty() && std::path::Path::new(&item.image_path).exists();
                if model.image_exists && !model.has_known_image_size() {
                    // Legacy rows may lack dimensions. One header read is
                    // cheaper than a preview that cannot compute a fit.
                    if let Ok((width, height)) = image::image_dimensions(&item.image_path) {
                        model.image_width = width;
                        model.image_height = height;
                    }
                }
                model.text.clear();
                model.chars = 0;
            }
            DisplayKind::File => {
                let data = FileData::from_json(&item.file_data);
                model.kind = PreviewKind::Files;
                model.total_files = data.files.len();
                model.files = data.files.into_iter().take(MAX_PREVIEW_FILES).collect();
                model.single_image_path = single_image_path(&model.files, model.total_files);
                model.text.clear();
                model.chars = 0;
            }
            _ => {
                model.kind = PreviewKind::Text;
            }
        }

        // The card masks phone/email/secret/link credentials; the preview must
        // not be a side channel that reveals them on hover.
        if model.kind == PreviewKind::Text
            && !item.meta_type.is_empty()
            && privacy_types.iter().any(|kind| kind == &item.meta_type)
        {
            model.text =
                sensitive_preview_to_text_with_types(&model.text, &item.meta_type, privacy_types);
            model.chars = model.text.chars().count();
        }

        model
    }

    /// Collapse a rich preview to its plain-text projection, matching the
    /// 「纯文本内容预览」card setting: HTML drops the styled document and keeps
    /// the same visible text the card shows, Markdown keeps its raw source
    /// instead of offering the rendered tab. Images and file lists are type
    /// previews rather than text styling, so they stay as they are.
    pub fn plain_text_only(mut self) -> Self {
        match self.kind {
            PreviewKind::Html => {
                // `text` already holds the visible projection (or the plain
                // fallback when the markup had none); only the rich document
                // and its degraded banner are dropped.
                self.kind = PreviewKind::Text;
                self.html = None;
                self.html_degraded = false;
            }
            PreviewKind::Markdown => self.kind = PreviewKind::Text,
            PreviewKind::Text | PreviewKind::Rtf | PreviewKind::Image | PreviewKind::Files => {}
        }
        self
    }

    pub fn is_image(&self) -> bool {
        self.kind == PreviewKind::Image
    }

    pub fn has_known_image_size(&self) -> bool {
        self.image_width > 0 && self.image_height > 0
    }
}

fn badge_label(kind: DisplayKind) -> &'static str {
    match kind {
        DisplayKind::Html => I18nKey::CardTypeHtml.text(),
        DisplayKind::Markdown => I18nKey::CardTypeMd.text(),
        DisplayKind::Rtf => I18nKey::CardTypeRtf.text(),
        DisplayKind::Email => I18nKey::CardTypeEmail.text(),
        DisplayKind::Phone => I18nKey::CardTypePhone.text(),
        DisplayKind::Link => I18nKey::CardTypeUrl.text(),
        DisplayKind::Path => I18nKey::CardTypePath.text(),
        DisplayKind::Color => I18nKey::CardTypeColor.text(),
        DisplayKind::Secret => I18nKey::CardTypeSecret.text(),
        DisplayKind::File => I18nKey::CardTypeFile.text(),
        DisplayKind::Image => I18nKey::CardTypeImage.text(),
        DisplayKind::PlainText => I18nKey::CardTypeText.text(),
    }
}

/// Prefer the OS plain-text representation, then decoded RTF. Mirrors the
/// card's defensive fallback chain without changing it.
fn plain_text_fallback(item: &ClipboardItem, rich: &RichData) -> String {
    if !item.full_text.trim().is_empty() {
        return item.full_text.clone();
    }
    rtf_plain_or_fallback(rich, item)
}

fn rtf_plain_or_fallback(rich: &RichData, item: &ClipboardItem) -> String {
    if let Some(rtf) = rich.rtf.as_deref().filter(|rtf| !rtf.trim().is_empty()) {
        let plain = rtf_text::rtf_to_plain_text(rtf);
        if !plain.is_empty() {
            return plain;
        }
    }
    item.full_text.clone()
}

/// A single file entry that is a regular image file. Multi-file groups stay a
/// list: turning one row into a picture would hide the other entries.
fn single_image_path(files: &[FileInfo], total_files: usize) -> Option<String> {
    if total_files != 1 {
        return None;
    }
    let file = files.first()?;
    if file.is_dir || !is_image_extension(&file.path) {
        return None;
    }
    Some(file.path.clone())
}

/// Count characters for the header. `chars()` matches what a user means by
/// "字数" for CJK text; byte length would overstate it.
pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// The short metadata line shared by the hover panel and the big window.
pub fn meta_text(model: &PreviewModel) -> String {
    match model.kind {
        PreviewKind::Text | PreviewKind::Markdown | PreviewKind::Rtf => {
            I18nKey::CardChars.fmt(&[&model.chars.to_string()])
        }
        PreviewKind::Html => {
            if model.html_degraded {
                I18nKey::PreviewTooLarge.text().to_string()
            } else {
                I18nKey::CardChars.fmt(&[&model.chars.to_string()])
            }
        }
        PreviewKind::Image => {
            let dimensions = if model.has_known_image_size() {
                format!("{} × {}", model.image_width, model.image_height)
            } else {
                I18nKey::PreviewUnknownSize.text().to_string()
            };
            if model.size > 0 {
                format!("{dimensions} · {}", format_size(model.size))
            } else {
                dimensions
            }
        }
        PreviewKind::Files => I18nKey::CardFilesCount.fmt(&[&model.total_files.to_string()]),
    }
}

/// Normalize CRLF/CR to LF so hover excerpts and line counts agree across
/// applications.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// First `max_lines` lines, capped at `max_chars` characters in total.
/// Returns the excerpt and whether anything was dropped.
pub fn excerpt(text: &str, max_lines: usize, max_chars: usize) -> (Vec<String>, bool) {
    let normalized = normalize_newlines(text);
    let total_chars = normalized.chars().count();
    if total_chars == 0 {
        return (Vec::new(), false);
    }

    let mut lines = Vec::new();
    let mut used = 0usize;
    let mut clipped = false;
    for line in normalized.split('\n') {
        if lines.len() >= max_lines || used >= max_chars {
            break;
        }
        let remaining = max_chars - used;
        let line_chars = line.chars().count();
        let take = line_chars.min(remaining);
        let mut clipped_line: String = line.chars().take(take).collect();
        if take < line_chars {
            clipped_line.push('…');
            clipped = true;
        }
        used += take;
        lines.push(clipped_line);
    }

    // Newline separators are not part of `used`, so comparing it against the
    // whole text's char count would flag every multi-line excerpt as dropped.
    let truncated = clipped || lines.len() < normalized.split('\n').count();
    (lines, truncated)
}

/// Escape a string for the `<pre>` fallback rendered through `TextView::html`.
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// A whitespace-preserving HTML document for plain text and RTF bodies.
pub fn plain_html_document(text: &str) -> String {
    format!("<pre>{}</pre>", escape_html(text))
}

/// Human-readable byte size. Reuses the card's formatter so both surfaces
/// agree on units and rounding.
pub fn format_size(bytes: i64) -> String {
    crate::ui::clipboard_card::format_file_size(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{ContentType, RichData};

    fn text_item(text: &str, meta: &str) -> ClipboardItem {
        let mut item = ClipboardItem::new_text(1, text, ContentType::PlainText, None, None);
        item.meta_type = meta.to_string();
        item
    }

    fn html_item(html: &str) -> ClipboardItem {
        let rich = RichData {
            html: Some(html.to_string()),
            ..Default::default()
        };
        ClipboardItem::new_text(
            2,
            "plain projection",
            ContentType::RichText,
            None,
            Some(&rich),
        )
    }

    fn file_item(paths: &[(&str, bool)]) -> ClipboardItem {
        let files: Vec<FileInfo> = paths
            .iter()
            .map(|(path, is_dir)| FileInfo {
                name: std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_string(),
                path: (*path).to_string(),
                is_dir: *is_dir,
            })
            .collect();
        let data = FileData {
            files,
            ..Default::default()
        };
        let mut item = ClipboardItem::new_text(3, "", ContentType::File, None, None);
        item.file_data = data.to_json();
        item
    }

    #[test]
    fn plain_text_stays_text() {
        let item = text_item("hello", "");
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Text);
        assert_eq!(model.text, "hello");
        assert_eq!(model.chars, 5);
    }

    #[test]
    fn markdown_is_detected_from_meta_type() {
        let item = text_item("# title", "markdown");
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Markdown);
        assert_eq!(model.text, "# title");
    }

    #[test]
    fn html_renders_under_the_limit() {
        let item = html_item("<p>hello</p>");
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Html);
        assert_eq!(model.html.as_deref(), Some("<p>hello</p>"));
        assert!(!model.html_degraded);
    }

    #[test]
    fn html_at_the_limit_is_allowed() {
        let body = "x".repeat(HTML_DEGRADE_BYTES);
        let item = html_item(&body);
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(
            model.html.as_deref().map(str::len),
            Some(HTML_DEGRADE_BYTES)
        );
        assert!(!model.html_degraded);
    }

    #[test]
    fn html_above_the_limit_degrades_to_plain_text() {
        let body = "x".repeat(HTML_DEGRADE_BYTES + 1);
        let item = html_item(&body);
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Text);
        assert!(model.html_degraded);
        assert!(model.html.is_none());
        assert_eq!(model.text, "plain projection");
    }

    #[test]
    fn plain_preview_drops_the_rich_html_document() {
        let item = html_item("<p>Hello <b>world</b></p>");
        let model = PreviewModel::from_item(&item, &[]).plain_text_only();
        assert_eq!(model.kind, PreviewKind::Text);
        assert!(model.html.is_none());
        assert!(!model.html_degraded);
        assert!(model.text.contains("Hello world"), "{}", model.text);
        assert!(!model.text.contains('<'), "{}", model.text);
    }

    #[test]
    fn plain_preview_keeps_the_markdown_source_instead_of_the_rich_tab() {
        let item = text_item("# title", "markdown");
        let model = PreviewModel::from_item(&item, &[]).plain_text_only();
        assert_eq!(model.kind, PreviewKind::Text);
        assert_eq!(model.text, "# title");
    }

    #[test]
    fn plain_preview_leaves_type_previews_alone() {
        let files = file_item(&[("C:/tmp/a.txt", false)]);
        assert_eq!(
            PreviewModel::from_item(&files, &[]).plain_text_only().kind,
            PreviewKind::Files
        );

        let image = ClipboardItem::new_text(5, "", ContentType::Image, None, None);
        assert_eq!(
            PreviewModel::from_item(&image, &[]).plain_text_only().kind,
            PreviewKind::Image
        );

        let rtf = RichData {
            rtf: Some(r"{\rtf1\ansi hello}".to_string()),
            ..Default::default()
        };
        let item = ClipboardItem::new_text(6, "fallback", ContentType::RichText, None, Some(&rtf));
        let model = PreviewModel::from_item(&item, &[]).plain_text_only();
        assert_eq!(model.kind, PreviewKind::Rtf);
        assert_eq!(model.text, "hello");
    }

    #[test]
    fn rtf_decodes_to_plain_text() {
        let rich = RichData {
            rtf: Some(r"{\rtf1\ansi hello}".to_string()),
            ..Default::default()
        };
        let item = ClipboardItem::new_text(4, "fallback", ContentType::RichText, None, Some(&rich));
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Rtf);
        assert_eq!(model.text, "hello");
    }

    #[test]
    fn files_are_truncated_but_total_is_kept() {
        let paths: Vec<(String, bool)> = (0..MAX_PREVIEW_FILES + 3)
            .map(|index| (format!("C:/tmp/file{index}.txt"), false))
            .collect();
        let refs: Vec<(&str, bool)> = paths
            .iter()
            .map(|(path, is_dir)| (path.as_str(), *is_dir))
            .collect();
        let item = file_item(&refs);
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.kind, PreviewKind::Files);
        assert_eq!(model.files.len(), MAX_PREVIEW_FILES);
        assert_eq!(model.total_files, MAX_PREVIEW_FILES + 3);
        assert!(model.single_image_path.is_none());
    }

    #[test]
    fn single_image_file_is_detected() {
        let item = file_item(&[("C:/tmp/photo.PNG", false)]);
        let model = PreviewModel::from_item(&item, &[]);
        assert_eq!(model.single_image_path.as_deref(), Some("C:/tmp/photo.PNG"));
    }

    #[test]
    fn directories_and_mixed_groups_are_not_single_images() {
        let dir = file_item(&[("C:/tmp/album.png", true)]);
        assert!(PreviewModel::from_item(&dir, &[])
            .single_image_path
            .is_none());

        let mixed = file_item(&[("C:/tmp/photo.png", false), ("C:/tmp/a.txt", false)]);
        assert!(PreviewModel::from_item(&mixed, &[])
            .single_image_path
            .is_none());
    }

    #[test]
    fn excerpt_normalizes_crlf_and_caps_both_limits() {
        let text = "one\r\ntwo\rthree\nfour\nfive";
        let (lines, truncated) = excerpt(text, 3, 100);
        assert_eq!(lines, vec!["one", "two", "three"]);
        assert!(truncated);

        let (lines, truncated) = excerpt("abcdefghij", 5, 4);
        assert_eq!(lines, vec!["abcd…"]);
        assert!(truncated);
    }

    #[test]
    fn fully_shown_multiline_excerpt_is_not_truncated() {
        let (lines, truncated) = excerpt("one\ntwo\nthree", 12, 900);
        assert_eq!(lines, vec!["one", "two", "three"]);
        assert!(!truncated);
    }

    #[test]
    fn excerpt_of_empty_text_is_empty() {
        let (lines, truncated) = excerpt("", 10, 100);
        assert!(lines.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn privacy_types_mask_text_like_the_card() {
        let item = text_item("sk-live-1234567890abcdef", "secret");
        let types = crate::core::settings::default_privacy_types();
        let masked = PreviewModel::from_item(&item, &types);
        assert_ne!(masked.text, "sk-live-1234567890abcdef");
        assert!(masked.chars > 0);

        let plain = PreviewModel::from_item(&item, &[]);
        assert_eq!(plain.text, "sk-live-1234567890abcdef");
    }

    #[test]
    fn plain_html_document_escapes_markup() {
        assert_eq!(
            plain_html_document("<b>&</b>"),
            "<pre>&lt;b&gt;&amp;&lt;/b&gt;</pre>"
        );
    }

    #[test]
    fn size_formatting_matches_the_card() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
    }
}
