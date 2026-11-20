//! Standalone preview window.
//!
//! The window is created on demand and destroyed when closed: GPUI 0.2.2 does
//! not expose cross-platform show/hide for an existing native window, and a
//! destroyed window cannot leave a stale handle behind. `WindowManager` owns
//! the lifetime and clears its references from `App::on_window_closed`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::text::{TextView, TextViewStyle};

use super::content::{self, PreviewKind, PreviewModel};
use crate::core::i18n_keys::I18nKey;
use crate::core::types::{ClipboardItem, FileInfo};
use crate::state::app::AppState;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Ctrl, IconSize, Radius, Space};
use crate::ui::design::typography::{self, type_role};
use crate::ui::rich_preview;
use crate::ui::widgets::{self, Tone};

const HEADER_HEIGHT: f32 = 48.;
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 8.0;
const ZOOM_STEP: f32 = 1.1;

pub enum PreviewWindowEvent {
    Close,
}

impl EventEmitter<PreviewWindowEvent> for PreviewWindowView {}

#[derive(Clone)]
struct FileRow {
    name: String,
    path: String,
    is_dir: bool,
    /// `None` while the background scan is still running.
    exists: Option<bool>,
    size: Option<u64>,
}

struct ScannedFiles {
    rows: Vec<FileRow>,
    single_image: Option<(String, u32, u32)>,
}

pub struct PreviewWindowView {
    state: Entity<AppState>,
    /// `Rc` keeps the per-frame clone cheap even for very large text bodies.
    model: Option<Rc<PreviewModel>>,
    file_rows: Vec<FileRow>,
    single_image: Option<(String, u32, u32)>,
    files_started: bool,
    files_generation: u64,
    _files_task: Option<Task<()>>,
    /// Markdown entries default to the original source, matching EcoPaste.
    md_view: bool,
    /// Prepared body source keyed by `(content_hash, markdown view)`. TextView
    /// skips re-parsing only while the source `SharedString` keeps its pointer
    /// identity, so rebuilding the string per frame reparses the whole
    /// document on every frame of the grow/shrink animation.
    body_source: Option<(u64, bool, SharedString)>,
    /// While the flyout card animates, the card owns the stroke and shadow and
    /// clips the content; at rest the view draws its own frame.
    draw_frame: bool,
    image_exists: bool,
    zoom: f32,
    pan: Point<f32>,
    drag: Option<(Point<Pixels>, Point<f32>)>,
    stage_bounds: Rc<Cell<Bounds<Pixels>>>,
    image_cache: Entity<RetainAllImageCache>,
    copied: bool,
    _copy_task: Option<Task<()>>,
    focus_handle: FocusHandle,
    focused_once: bool,
    /// Embedded in the non-activating hover flyout instead of the standalone
    /// window: never take focus and hide the close button (Esc/leave closes).
    embedded: bool,
}

impl PreviewWindowView {
    pub fn new(
        state: Entity<AppState>,
        item: ClipboardItem,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> Self {
        let embedded = window.is_none();
        let privacy = state.read(cx).settings.privacy_protection_types.clone();
        let model = PreviewModel::from_item(&item, &privacy);
        let image_exists = model.image_exists;
        let focus_handle = cx.focus_handle();
        let view = Self {
            state,
            model: Some(Rc::new(model)),
            file_rows: Vec::new(),
            single_image: None,
            files_started: false,
            files_generation: 0,
            _files_task: None,
            md_view: false,
            body_source: None,
            draw_frame: true,
            image_exists,
            zoom: 1.0,
            pan: point(0f32, 0f32),
            drag: None,
            stage_bounds: Rc::new(Cell::new(Bounds::default())),
            image_cache: RetainAllImageCache::new(cx),
            copied: false,
            _copy_task: None,
            focus_handle,
            // Embedded views must never focus; the standalone window focuses
            // its root so Esc works.
            focused_once: embedded,
            embedded,
        };
        if let Some(window) = window {
            window.focus(&view.focus_handle, cx);
        }
        view
    }

    pub fn set_item(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        let privacy = self
            .state
            .read(cx)
            .settings
            .privacy_protection_types
            .clone();
        let model = PreviewModel::from_item(&item, &privacy);
        self.image_exists = model.image_exists;
        self.model = Some(Rc::new(model));
        self.file_rows.clear();
        self.single_image = None;
        self.files_started = false;
        self.files_generation = self.files_generation.wrapping_add(1);
        self.md_view = false;
        self.body_source = None;
        self.zoom = 1.0;
        self.pan = point(0f32, 0f32);
        self.drag = None;
        self.copied = false;
        self._copy_task = None;
        self.image_cache = RetainAllImageCache::new(cx);
        cx.notify();
    }

    pub fn item_id(&self) -> Option<i64> {
        self.model.as_ref().map(|model| model.id)
    }

    /// The flyout card draws the stroke/shadow while its rect animates and
    /// clips this view; hand the frame back once the card settles.
    pub fn set_draw_frame(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.draw_frame != on {
            self.draw_frame = on;
            cx.notify();
        }
    }

    fn reset_image(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = point(0f32, 0f32);
        self.drag = None;
        cx.notify();
    }

    /// Kick off the one-time filesystem scan for file entries. Called from
    /// `render` because the entity handle is only available after `new`.
    fn ensure_files_scan(&mut self, cx: &mut Context<Self>) {
        if self.files_started {
            return;
        }
        let Some(model) = self.model.as_ref() else {
            return;
        };
        if model.kind != PreviewKind::Files {
            return;
        }
        self.files_started = true;
        self.files_generation = self.files_generation.wrapping_add(1);
        let generation = self.files_generation;
        let files = model.files.clone();
        let single = model.single_image_path.clone();
        self.file_rows = files
            .iter()
            .map(|file| FileRow {
                name: file.name.clone(),
                path: file.path.clone(),
                is_dir: file.is_dir,
                exists: None,
                size: None,
            })
            .collect();

        self._files_task = Some(cx.spawn(async move |this, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move { scan_files(files, single) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.files_generation == generation {
                    this.file_rows = scanned.rows;
                    this.single_image = scanned.single_image;
                    cx.notify();
                }
            });
            // `cached_file_icon_path` starts background icon jobs on the first
            // render after the scan. One delayed repaint picks up whatever
            // finished, so rows do not stay on the generic glyph forever.
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    fn copy_item(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.item_id() else {
            return;
        };
        let plain = self.state.read(cx).settings.copy_as_plain_text;
        self.state
            .update(cx, |state, _cx| state.copy_item(id, plain));
        self.copied = true;
        self._copy_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1000))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn render_header(
        &mut self,
        model: &PreviewModel,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = model.id;
        let badge = model.badge;
        let meta = content::meta_text(model);
        let show_md_tabs = model.kind == PreviewKind::Markdown;
        let md_view = self.md_view;
        let this_for_close = cx.entity();
        let this_for_copy = cx.entity();

        div()
            .h(px(HEADER_HEIGHT))
            .flex_shrink_0()
            .px(Space::Xl.scaled())
            .flex()
            .items_center()
            .gap(Space::Md.scaled())
            .border_b(px(1.))
            .border_color(p.stroke)
            .child(
                div()
                    .px(px(6.))
                    .rounded(Radius::Sm.scaled())
                    .bg(p.tag_bg)
                    .text_size(typography::font(type_role::CAPTION))
                    .text_color(p.tag_ink)
                    .child(badge),
            )
            .child(
                div()
                    .text_size(typography::font(type_role::BASE))
                    .font_weight(typography::LABEL_WEIGHT)
                    .text_color(p.ink)
                    .child(I18nKey::PreviewTitle.text()),
            )
            .child(
                div()
                    .truncate()
                    .text_size(typography::font(type_role::CAPTION))
                    .text_color(p.ink_3)
                    .child(meta),
            )
            // The middle strip is the native drag area; buttons deliberately
            // live outside it so a drag never swallows a click.
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .when(show_md_tabs, |header| {
                header.child(
                    div()
                        .h(Ctrl::Chip.scaled())
                        .w(px(96.))
                        .p(px(2.))
                        .rounded(Radius::Md.scaled())
                        .bg(p.control_face)
                        .flex()
                        .child(
                            widgets::chip(
                                p,
                                ElementId::Name(format!("preview-md-{id}").into()),
                                md_view,
                            )
                            .child(I18nKey::CardTypeMd.text())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.md_view = true;
                                cx.notify();
                            })),
                        )
                        .child(
                            widgets::chip(
                                p,
                                ElementId::Name(format!("preview-source-{id}").into()),
                                !md_view,
                            )
                            .child(I18nKey::PreviewOriginal.text())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.md_view = false;
                                cx.notify();
                            })),
                        ),
                )
            })
            .child(
                widgets::btn_small(
                    p,
                    ElementId::Name(format!("preview-window-copy-{id}").into()),
                    if self.copied {
                        I18nKey::PreviewCopied.text()
                    } else {
                        I18nKey::PreviewCopy.text()
                    },
                    Some(if self.copied { "CheckIcon" } else { "CopyIcon" }),
                    Tone::Plain,
                )
                .on_click(move |_, _, cx| {
                    this_for_copy.update(cx, |view, cx| view.copy_item(cx));
                }),
            )
            .when(!self.embedded, |header| {
                header.child(
                    widgets::ghost_icon(
                        p,
                        ElementId::Name(format!("preview-window-close-{id}").into()),
                        "CloseIcon",
                        Ctrl::Small,
                        IconSize::Sm,
                        p.ink_2,
                    )
                    .on_click(move |_, _, cx| {
                        this_for_close.update(cx, |_, cx| {
                            cx.emit(PreviewWindowEvent::Close);
                        });
                    }),
                )
            })
            .into_any_element()
    }

    fn render_text_body(&mut self, model: &PreviewModel, p: &Palette) -> AnyElement {
        // Component `TextViewStyle` derives colors from the active theme, so
        // only layout knobs live here. Keep the default heading scale: the
        // window is a reading surface, unlike the compact card.
        let style = TextViewStyle::default().paragraph_gap(rems(0.5));
        let key = model.content_hash;
        let md_view = model.kind == PreviewKind::Markdown && self.md_view;

        // Reuse the same allocation across frames: TextView compares the
        // source by pointer and reparses the document whenever it changes.
        let source = match &self.body_source {
            Some((cached_key, cached_md, source))
                if *cached_key == key && *cached_md == md_view =>
            {
                source.clone()
            }
            _ => {
                let source = SharedString::from(prepare_body_source(model, md_view));
                self.body_source = Some((key, md_view, source.clone()));
                source
            }
        };

        let text_view = if md_view {
            TextView::markdown(("preview-window-md", key), source)
        } else {
            TextView::html(("preview-window-html", key), source)
        };

        let mut column = div().flex().flex_col().flex_1().min_h(px(0.)).w_full();
        if model.html_degraded {
            column = column.child(
                div()
                    .mx(Space::Xl.scaled())
                    .mt(Space::Lg.scaled())
                    .px(Space::Md.scaled())
                    .py(Space::Sm.scaled())
                    .rounded(Radius::Md.scaled())
                    .bg(p.control_face)
                    .text_size(typography::font(type_role::CAPTION))
                    .text_color(p.ink_2)
                    .child(I18nKey::PreviewTooLarge.text()),
            );
        }
        column
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .px(Space::Xl.scaled())
                    .py(Space::Lg.scaled())
                    .text_size(typography::font(type_role::BASE))
                    .text_color(p.ink)
                    .child(
                        text_view
                            .style(style)
                            .selectable(true)
                            .scrollable(true)
                            .w_full()
                            .h_full(),
                    ),
            )
            .into_any_element()
    }

    fn render_image_body(
        &mut self,
        path: &str,
        width: u32,
        height: u32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.image_exists {
            return empty_state(p, I18nKey::PreviewImageMissing.text());
        }
        if width == 0 || height == 0 {
            // File exists but the header could not be decoded.
            return empty_state(p, I18nKey::PreviewUnknownSize.text());
        }
        let stage = stage_size_f32(self.stage_bounds.get().size);
        let image = size(width as f32, height as f32);
        let fit = fit_size(image, stage);
        let zoom = clamp_zoom(self.zoom);
        let scaled = size(fit.width * zoom, fit.height * zoom);
        let pan = clamp_pan(self.pan, scaled, stage);
        let left = (stage.width - scaled.width) * 0.5 + pan.x;
        let top = (stage.height - scaled.height) * 0.5 + pan.y;
        let stage_tracker = self.stage_bounds.clone();
        let stage_view = cx.entity();
        let image_cache = self.image_cache.clone();
        let path = path.to_string();
        let draggable = zoom > 1.0;

        div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .bg(p.media_scrim)
            .cursor(if draggable {
                CursorStyle::OpenHand
            } else {
                CursorStyle::Arrow
            })
            .on_children_prepainted(move |bounds, _, cx| {
                if let Some(bounds) = bounds.first() {
                    if stage_tracker.get() != *bounds {
                        stage_tracker.set(*bounds);
                        // The first paint has no stage size; ask for one more
                        // frame so the fitted (and centered) image appears
                        // immediately instead of at 0×0.
                        stage_view.update(cx, |_, cx| cx.notify());
                    }
                }
            })
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                if !this.image_exists {
                    return;
                }
                let delta = match event.delta {
                    ScrollDelta::Lines(delta) => delta.y,
                    ScrollDelta::Pixels(delta) => f32::from(delta.y) / 40.0,
                };
                if delta.abs() < 0.01 {
                    return;
                }
                let stage_bounds = this.stage_bounds.get();
                let stage = stage_size_f32(stage_bounds.size);
                if stage.width <= 0. || stage.height <= 0. {
                    return;
                }
                let old_zoom = clamp_zoom(this.zoom);
                let new_zoom = clamp_zoom(old_zoom * ZOOM_STEP.powf(delta.clamp(-3., 3.)));
                if (new_zoom - old_zoom).abs() < 0.001 {
                    return;
                }
                let model = this.model.as_ref();
                let dims = if let Some(model) = model {
                    if model.has_known_image_size() {
                        size(model.image_width as f32, model.image_height as f32)
                    } else if let Some((_, width, height)) = this.single_image.as_ref() {
                        size(*width as f32, *height as f32)
                    } else {
                        return;
                    }
                } else {
                    return;
                };
                let fit = fit_size(dims, stage);
                let scaled_after = size(fit.width * new_zoom, fit.height * new_zoom);
                let anchor = point(
                    f32::from(event.position.x - stage_bounds.origin.x) - stage.width * 0.5,
                    f32::from(event.position.y - stage_bounds.origin.y) - stage.height * 0.5,
                );
                this.pan = clamp_pan(
                    zoom_around(anchor, this.pan, old_zoom, new_zoom),
                    scaled_after,
                    stage,
                );
                this.zoom = new_zoom;
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        this.reset_image(cx);
                        cx.stop_propagation();
                        return;
                    }
                    if this.zoom <= 1.0 {
                        return;
                    }
                    this.drag = Some((event.position, this.pan));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some((start, start_pan)) = this.drag else {
                    return;
                };
                let stage = stage_size_f32(this.stage_bounds.get().size);
                let model = this.model.as_ref();
                let dims = if let Some(model) = model {
                    if model.has_known_image_size() {
                        size(model.image_width as f32, model.image_height as f32)
                    } else if let Some((_, width, height)) = this.single_image.as_ref() {
                        size(*width as f32, *height as f32)
                    } else {
                        return;
                    }
                } else {
                    return;
                };
                let fit = fit_size(dims, stage);
                let scaled = size(fit.width * this.zoom, fit.height * this.zoom);
                let delta = event.position - start;
                this.pan = clamp_pan(
                    point(
                        start_pan.x + f32::from(delta.x),
                        start_pan.y + f32::from(delta.y),
                    ),
                    scaled,
                    stage,
                );
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(
                div().absolute().inset_0().child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(scaled.width))
                        .h(px(scaled.height))
                        .child(
                            img(std::path::PathBuf::from(path))
                                .w_full()
                                .h_full()
                                .object_fit(ObjectFit::Fill)
                                .image_cache(&image_cache),
                        ),
                ),
            )
            .child(
                div()
                    .absolute()
                    .right(Space::Lg.scaled())
                    .bottom(Space::Lg.scaled())
                    .flex()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .px(Space::Md.scaled())
                    .py(px(4.))
                    .rounded(Radius::Lg.scaled())
                    .bg(p.flyout)
                    .border(px(1.))
                    .border_color(p.stroke)
                    .shadow(widgets::flyout_shadow(p.dark))
                    .child({
                        let volume = format!("{}", (zoom * 100.0).round() as i32);
                        div()
                            .text_size(typography::font(type_role::CAPTION))
                            .text_color(p.ink_2)
                            .child(I18nKey::PreviewZoom.fmt(&[&volume]))
                    })
                    .child(
                        widgets::btn_small(
                            p,
                            "preview-image-reset",
                            I18nKey::PreviewResetZoom.text(),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.reset_image(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_files_body(&self, model: &PreviewModel, p: &Palette) -> AnyElement {
        if model.files.is_empty() && model.total_files == 0 {
            return empty_state(p, I18nKey::PreviewEmpty.text());
        }
        let pending = self.file_rows.iter().any(|row| row.exists.is_none());
        let mut rows = div().flex().flex_col().py(Space::Sm.scaled());
        for row in &self.file_rows {
            let missing = row.exists == Some(false);
            let icon = crate::ui::clipboard_card::cached_file_icon_path(&row.path, row.is_dir);
            let size_text = if missing {
                I18nKey::PreviewFileMissing.text().to_string()
            } else if row.is_dir {
                I18nKey::CardTypeFolder.text().to_string()
            } else if row.exists.is_none() {
                I18nKey::PreviewLoading.text().to_string()
            } else {
                row.size
                    .map(|size| content::format_size(size as i64))
                    .unwrap_or_default()
            };
            let size_color = if missing { p.warn } else { p.ink_3 };

            rows = rows.child(
                div()
                    .h(px(40.))
                    .w_full()
                    .px(Space::Xl.scaled())
                    .flex()
                    .items_center()
                    .gap(Space::Md.scaled())
                    .when(missing, |row| row.opacity(0.55))
                    .child(match icon {
                        Some(path) => img(path).size(IconSize::Md.scaled()).into_any_element(),
                        None => widgets::icon(
                            if row.is_dir { "PathIcon" } else { "FilesIcon" },
                            IconSize::Md.scaled(),
                            p.ink_2,
                        )
                        .into_any_element(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(typography::font(type_role::BODY))
                                    .text_color(p.ink)
                                    .child(row.name.clone()),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(typography::font(type_role::CAPTION))
                                    .text_color(p.ink_3)
                                    .child(row.path.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(typography::font(type_role::CAPTION))
                            .text_color(size_color)
                            .child(size_text),
                    ),
            );
        }

        let truncated = model.total_files > model.files.len();
        div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .child(rows),
            )
            .when(truncated, |body| {
                body.child(
                    div()
                        .flex_shrink_0()
                        .px(Space::Xl.scaled())
                        .py(Space::Sm.scaled())
                        .border_t(px(1.))
                        .border_color(p.stroke)
                        .text_size(typography::font(type_role::CAPTION))
                        .text_color(p.ink_3)
                        .child(I18nKey::PreviewFilesTruncated.fmt(&[
                            &model.files.len().to_string(),
                            &model.total_files.to_string(),
                        ])),
                )
            })
            .when(pending, |body| {
                body.child(
                    div()
                        .flex_shrink_0()
                        .px(Space::Xl.scaled())
                        .pb(Space::Sm.scaled())
                        .text_size(typography::font(type_role::CAPTION))
                        .text_color(p.ink_3)
                        .child(I18nKey::PreviewLoading.text()),
                )
            })
            .into_any_element()
    }
}

impl Render for PreviewWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.embedded && !self.focused_once {
            self.focused_once = true;
            self.focus_handle.focus(window, cx);
        }

        self.ensure_files_scan(cx);

        let Some(model) = self.model.clone() else {
            return empty_state(&Palette::current(cx), I18nKey::PreviewEmpty.text());
        };
        let p = Palette::current(cx);
        let model_kind = model.kind;
        let model_id = model.id;
        let single_image = self.single_image.clone();
        let image_path = model.image_path.clone();
        let image_width = model.image_width;
        let image_height = model.image_height;

        let header = self.render_header(&model, &p, cx);
        let body = match model_kind {
            PreviewKind::Image => {
                self.render_image_body(&image_path, image_width, image_height, &p, cx)
            }
            PreviewKind::Files => match single_image {
                // The background scan only publishes a single image after
                // `image::image_dimensions` succeeded, so it is safe to treat
                // the file as present without another stat.
                Some((path, width, height)) => {
                    self.image_exists = true;
                    self.render_image_body(&path, width, height, &p, cx)
                }
                None => self.render_files_body(&model, &p),
            },
            _ => self.render_text_body(&model, &p),
        };

        // The flyout canvas is transparent and frameless; the card draws its
        // own rounded edge, stroke and shadow so they move with the content.
        div()
            .id(ElementId::Name(
                format!("preview-window-root-{model_id}").into(),
            ))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .text_color(p.ink)
            .rounded(px(crate::core::frontend::PANEL_RADIUS))
            .when(self.draw_frame, |root| {
                root.bg(p.background)
                    .border(px(1.))
                    .border_color(p.stroke)
                    .when(cfg!(target_os = "windows"), |root| {
                        root.shadow(widgets::flyout_shadow(p.dark))
                    })
            })
            .track_focus(&self.focus_handle)
            // Capture so Esc closes even when a selectable text view owns
            // focus and would otherwise consume the key.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    cx.emit(PreviewWindowEvent::Close);
                    cx.stop_propagation();
                    return;
                }
                let image_view = this.model.as_ref().is_some_and(|model| model.is_image())
                    || this.single_image.is_some();
                if image_view {
                    match key {
                        "+" | "=" => {
                            this.zoom = clamp_zoom(this.zoom * ZOOM_STEP);
                            let stage = stage_size_f32(this.stage_bounds.get().size);
                            this.pan = clamp_pan(this.pan, scaled_image(this), stage);
                            cx.notify();
                        }
                        "-" => {
                            this.zoom = clamp_zoom(this.zoom / ZOOM_STEP);
                            let stage = stage_size_f32(this.stage_bounds.get().size);
                            this.pan = clamp_pan(this.pan, scaled_image(this), stage);
                            cx.notify();
                        }
                        "0" => this.reset_image(cx),
                        _ => {}
                    }
                }
            }))
            .child(header)
            .child(body)
            .into_any_element()
    }
}

fn empty_state(p: &Palette, text: &str) -> AnyElement {
    div()
        .flex_1()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(typography::font(type_role::BODY))
        .text_color(p.ink_3)
        .child(text.to_string())
        .into_any_element()
}

/// `TextView::html` expects a document; wrap plain text in a whitespace-
/// preserving block so RTF/plain/degraded bodies render exactly as captured.
fn plain_document(text: &str) -> String {
    content::plain_html_document(text)
}

/// The document handed to the full preview's `TextView`, prepared once per
/// item (and per Markdown tab) instead of once per frame.
fn prepare_body_source(model: &PreviewModel, md_view: bool) -> String {
    match model.kind {
        PreviewKind::Markdown if md_view => rich_preview::strip_markdown_links(&model.text),
        PreviewKind::Markdown => plain_document(&model.text),
        PreviewKind::Html => model
            .html
            .as_deref()
            .map(rich_preview::normalize_clipboard_html_for_render)
            .map(|html| rich_preview::strip_html_links(&html))
            .unwrap_or_else(|| plain_document(&model.text)),
        _ => plain_document(&model.text),
    }
}

fn scan_files(files: Vec<FileInfo>, single: Option<String>) -> ScannedFiles {
    let rows = files
        .iter()
        .map(|file| {
            let metadata = std::fs::metadata(&file.path).ok();
            FileRow {
                name: file.name.clone(),
                path: file.path.clone(),
                is_dir: file.is_dir,
                exists: Some(metadata.is_some()),
                size: if file.is_dir {
                    None
                } else {
                    metadata.map(|meta| meta.len())
                },
            }
        })
        .collect();

    let single_image = single.and_then(|path| {
        let dimensions = image::image_dimensions(&path).ok()?;
        Some((path, dimensions.0, dimensions.1))
    });

    ScannedFiles { rows, single_image }
}

fn scaled_image(view: &PreviewWindowView) -> Size<f32> {
    let Some(model) = view.model.as_ref() else {
        return size(0., 0.);
    };
    let dims = if model.has_known_image_size() {
        size(model.image_width as f32, model.image_height as f32)
    } else if let Some((_, width, height)) = view.single_image.as_ref() {
        size(*width as f32, *height as f32)
    } else {
        return size(0., 0.);
    };
    let stage = stage_size_f32(view.stage_bounds.get().size);
    let fit = fit_size(dims, stage);
    size(fit.width * view.zoom, fit.height * view.zoom)
}

fn stage_size_f32(stage: Size<Pixels>) -> Size<f32> {
    size(f32::from(stage.width), f32::from(stage.height))
}

pub(crate) fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Largest image rectangle that fits inside `stage`, preserving aspect ratio.
pub(crate) fn fit_size(image: Size<f32>, stage: Size<f32>) -> Size<f32> {
    if image.width <= 0. || image.height <= 0. || stage.width <= 0. || stage.height <= 0. {
        return size(0., 0.);
    }
    let scale = (stage.width / image.width).min(stage.height / image.height);
    size(image.width * scale, image.height * scale)
}

/// Keep panning bounded so the stage never reveals empty background at the
/// edges. When the scaled image is smaller than the stage, centering wins.
pub(crate) fn clamp_pan(pan: Point<f32>, scaled: Size<f32>, stage: Size<f32>) -> Point<f32> {
    let max_x = ((scaled.width - stage.width) * 0.5).max(0.);
    let max_y = ((scaled.height - stage.height) * 0.5).max(0.);
    point(pan.x.clamp(-max_x, max_x), pan.y.clamp(-max_y, max_y))
}

/// Keep the point under the pointer fixed while zoom changes.
pub(crate) fn zoom_around(
    anchor: Point<f32>,
    pan: Point<f32>,
    old_zoom: f32,
    new_zoom: f32,
) -> Point<f32> {
    if old_zoom <= 0. {
        return pan;
    }
    let ratio = new_zoom / old_zoom;
    point(
        anchor.x - (anchor.x - pan.x) * ratio,
        anchor.y - (anchor.y - pan.y) * ratio,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_pan, clamp_zoom, fit_size, prepare_body_source, zoom_around, PreviewKind,
        PreviewModel, ZOOM_MAX, ZOOM_MIN,
    };
    use crate::core::types::{ClipboardItem, ContentType};
    use gpui::{point, size};

    fn model(kind: PreviewKind) -> PreviewModel {
        let item = ClipboardItem::new_text(1, "hello", ContentType::PlainText, None, None);
        let mut model = PreviewModel::from_item(&item, &[]);
        model.kind = kind;
        model
    }

    #[test]
    fn html_body_source_keeps_text_but_drops_link_urls() {
        let mut model = model(PreviewKind::Html);
        model.html = Some(r#"<p>keep <a href="https://example.com">link</a> text</p>"#.to_string());
        let source = prepare_body_source(&model, false);
        assert!(source.contains("link"));
        assert!(!source.contains("href"));
    }

    #[test]
    fn markdown_tab_strips_links_while_the_source_tab_keeps_them() {
        let mut model = model(PreviewKind::Markdown);
        model.text = "see [docs](https://example.com) now".to_string();
        assert!(!prepare_body_source(&model, true).contains("https://"));
        assert!(prepare_body_source(&model, false).contains("https://"));
    }

    #[test]
    fn plain_body_source_preserves_the_text() {
        let model = model(PreviewKind::Text);
        assert!(prepare_body_source(&model, false).contains("hello"));
    }

    #[test]
    fn fit_keeps_aspect_and_fits_inside() {
        let fitted = fit_size(size(400., 200.), size(200., 200.));
        assert_eq!(fitted, size(200., 100.));

        let fitted = fit_size(size(100., 400.), size(400., 200.));
        assert_eq!(fitted, size(50., 200.));
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        assert_eq!(clamp_zoom(0.01), ZOOM_MIN);
        assert_eq!(clamp_zoom(99.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(1.0), 1.0);
    }

    #[test]
    fn pan_is_clamped_to_the_scaled_image() {
        let stage = size(200., 100.);
        let scaled = size(400., 200.);
        assert_eq!(
            clamp_pan(point(500., -500.), scaled, stage),
            point(100., -50.)
        );

        let smaller = size(100., 50.);
        assert_eq!(clamp_pan(point(20., 20.), smaller, stage), point(0., 0.));
    }

    #[test]
    fn zooming_keeps_the_pointer_anchor_stable() {
        // Anchor 10 px right of center; pan 0, zoom doubles. The point under
        // the anchor moves from +10 to +20 relative to the center, so pan must
        // move to -10 to keep it under the pointer.
        let pan = zoom_around(point(10., 0.), point(0., 0.), 1.0, 2.0);
        assert_eq!(pan, point(-10., 0.));

        // Zooming back restores the original pan.
        let restored = zoom_around(point(10., 0.), pan, 2.0, 1.0);
        assert_eq!(restored, point(0., 0.));
    }
}
