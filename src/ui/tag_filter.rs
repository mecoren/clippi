//! Tag filter panel — floating panel for filtering clipboard items by tag.
//!
//! --- Matches the original Slint TagFilterPanel.slint design: ---
//! --- - Title row with AND/OR mode toggle, clear, close buttons ---
//! --- - Tag creation input with "+" button (real gpui_component Input) ---
//! --- - 2-column tag grid (140px each, 30px height) ---
//! --- - Edit/delete buttons per tag row ---
//! - TagEditPanel overlay for editing name/color

use std::rc::Rc;

use gpui::prelude::*;
use gpui::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};

use crate::core::i18n_keys::I18nKey;
use crate::core::types::{tag_preset_colors, TagInfo};
use crate::state::app::AppState;

use super::clipboard_list::ClipboardListView;
use super::components::reorder::{
    drag_source, drop_target, DragAutoScroll, ReorderDrag, ReorderKey,
};
use super::design::palette::Palette;
use super::design::size::{Ctrl, Radius, Space};
use super::design::typography::{self, type_role};
use super::filter_bar::FilterBar;
use super::widgets;

/// Pending tag-deletion target, emitted as a user intent and confirmed at the
/// root level before any database mutation happens.
#[derive(Clone, Debug)]
pub(crate) struct TagDeleteTarget {
    pub id: i64,
    pub name: String,
}

/// Events emitted by the tag filter panel.
#[derive(Clone, Debug)]
pub(crate) enum TagFilterEvent {
    /// The user clicked a tag's trash button — root should show the
    /// confirmation dialog instead of deleting immediately.
    RequestDeleteTag(TagDeleteTarget),
}

impl EventEmitter<TagFilterEvent> for TagFilterPanel {}

pub struct TagFilterPanel {
    state: Entity<AppState>,
    list_view: Entity<ClipboardListView>,
    filter_bar: Entity<FilterBar>,
    create_input: Entity<InputState>,
    edit_name_input: Entity<InputState>,
    /// Tag grid scroll offset, driven so a reorder drag can auto-scroll it.
    scroll: DragAutoScroll,
    last_edit_tag_id: i64,
    last_lang_version: u64,
}

impl TagFilterPanel {
    pub fn new(
        state: Entity<AppState>,
        list_view: Entity<ClipboardListView>,
        filter_bar: Entity<FilterBar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let create_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(I18nKey::TagCreatePlaceholder.text())
        });
        let edit_name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(I18nKey::TagCreatePlaceholder.text())
        });

        Self {
            state,
            list_view,
            filter_bar,
            create_input,
            edit_name_input,
            scroll: DragAutoScroll::new(),
            last_edit_tag_id: -1,
            last_lang_version: crate::core::i18n::lang_version(),
        }
    }

    pub(crate) fn refresh_list(&self, cx: &mut App) {
        let items = self.state.read(cx).visible_items();
        self.list_view.update(cx, |l, cx| l.set_items(items, cx));
        self.filter_bar.update(cx, |_b, cx| cx.notify());
    }

    pub(crate) fn cancel_edit_tag(&self, cx: &mut App) {
        self.state.update(cx, |s, _cx| s.cancel_edit_tag());
    }

    pub(crate) fn set_edit_tag_color(&self, color: &str, cx: &mut App) {
        self.state.update(cx, |s, _cx| s.set_edit_tag_color(color));
    }

    pub(crate) fn update_tag(&self, tag_id: i64, name: &str, color: &str, cx: &mut App) {
        self.state.update(cx, |s, cx| {
            s.update_tag(tag_id, name, color);
            cx.notify();
        });
        self.refresh_list(cx);
    }

    fn toggle_filter(&self, tag_id: i64, cx: &mut App) {
        self.state.update(cx, |s, cx| {
            s.toggle_tag_filter(tag_id);
            cx.notify();
        });
        self.refresh_list(cx);
    }

    fn clear_filters(&self, cx: &mut App) {
        self.state.update(cx, |s, cx| {
            s.clear_tag_filters();
            cx.notify();
        });
        self.refresh_list(cx);
    }

    fn toggle_mode(&self, cx: &mut App) {
        self.state.update(cx, |s, cx| {
            s.toggle_tag_match_mode();
            cx.notify();
        });
        self.refresh_list(cx);
    }

    fn close(&self, cx: &mut App) {
        self.filter_bar
            .update(cx, |bar, cx| bar.close_tag_panel(cx));
    }

    fn create_tag(&self, name: &str, cx: &mut App) {
        self.state.update(cx, |s, cx| {
            s.create_tag(name);
            cx.notify();
        });
        self.refresh_list(cx);
    }

    /// Read the create input value.
    /// If the name exactly matches an existing tag → toggle its filter;
    /// otherwise create a new tag. Clear the input afterwards.
    fn create_tag_from_input(&self, window: &mut Window, cx: &mut App) {
        let name = self.create_input.read(cx).value().to_string();
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            let tag_id = {
                let app_state = self.state.read(cx);
                app_state
                    .tags
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(trimmed))
                    .map(|t| t.id)
            };
            if let Some(tag_id) = tag_id {
                self.toggle_filter(tag_id, cx);
            } else {
                self.create_tag(trimmed, cx);
            }
        }
        self.create_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    pub fn edit_name_input(&self) -> &Entity<InputState> {
        &self.edit_name_input
    }
}

impl Render for TagFilterPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 语言切换时刷新 InputState placeholder
        let current = crate::core::i18n::lang_version();
        if self.last_lang_version != current {
            self.last_lang_version = current;
            self.create_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::TagCreatePlaceholder.text(), window, cx);
            });
            self.edit_name_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::TagCreatePlaceholder.text(), window, cx);
            });
        }

        let app_state = self.state.read(cx);
        // --- Filter tags by input text for live search (pinyin-aware) ---
        let filter_text = self.create_input.read(cx).value();
        let pinned_tag_ids = app_state.settings.pinned_tag_ids.clone();
        let tags: Vec<(TagInfo, bool, bool)> = app_state
            .tags
            .iter()
            .filter(|t| {
                filter_text.is_empty()
                    || crate::core::search::text_matches_term(&t.name, &filter_text)
            })
            .map(|t| {
                (
                    t.clone(),
                    app_state.filters.tag_ids.contains(&t.id),
                    pinned_tag_ids.contains(&t.id),
                )
            })
            .collect();
        let tag_match_all = app_state.filters.is_tag_match_all();
        let has_tag_filter = !app_state.filters.tag_ids.is_empty();
        let _ = app_state;

        // 浮层面板：不透明 flyout 底 + stroke_strong 描边，不投影。
        let p = Palette::current(cx);
        let accent = Rgba::from(p.accent);
        let text_1 = Rgba::from(p.ink);
        let text_2 = Rgba::from(p.ink_2);
        let text_3 = Rgba::from(p.ink_3);
        let surface = Rgba::from(p.flyout);
        let input_bg = Rgba::from(p.on_card("control"));
        let sep_line = Rgba::from(p.stroke);
        let btn_hover = Rgba::from(p.on_card("hover"));
        let panel_border = Rgba::from(p.stroke_strong);
        let active_bg = Rgba::from(p.accent.opacity(0.13));
        let danger_hover_bg = Rgba::from(p.danger.opacity(0.18));

        let rows: Vec<Vec<(TagInfo, bool, bool)>> = tags.chunks(2).map(|r| r.to_vec()).collect();
        let rows_is_empty = rows.is_empty();
        let this_entity = cx.entity().clone();

        div()
            .flex()
            .flex_col()
            .w(px(304.))
            .max_h(px(360.))
            .bg(surface)
            .border_color(panel_border)
            .border_1()
            .rounded(Radius::Lg.scaled())
            .pt(Space::Md.scaled())
            .pb(Space::Md.scaled())
            .pl(Space::Md.scaled())
            .gap(Space::Xs.scaled())
            // --- Title row ---
            .child({
                let this = this_entity.clone();
                div()
                    .pr(px(8.))
                    .h(px(24.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .text_size(typography::font(type_role::BASE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_1)
                            .child(I18nKey::TagFilterTitle.text()),
                    )
                    .child(div().flex_1())
                    .child(icon_btn(
                        "tag-filter-mode",
                        "CombineIcon",
                        if tag_match_all { accent } else { text_2 },
                        btn_hover,
                        Some(I18nKey::TagTooltipToggleMode.text()),
                        {
                            let this = this.clone();
                            move |_, cx| this.update(cx, |panel, cx| panel.toggle_mode(cx))
                        },
                    ))
                    .when(has_tag_filter, |el| {
                        el.child(icon_btn(
                            "tag-filter-clear",
                            "ClearIcon",
                            text_2,
                            btn_hover,
                            Some(I18nKey::TagTooltipClear.text()),
                            {
                                let this = this.clone();
                                move |_, cx| this.update(cx, |panel, cx| panel.clear_filters(cx))
                            },
                        ))
                    })
                    .child(icon_btn(
                        "tag-filter-close",
                        "CloseIcon",
                        text_2,
                        btn_hover,
                        None,
                        {
                            let this = this.clone();
                            move |_, cx| this.update(cx, |panel, cx| panel.close(cx))
                        },
                    ))
            })
            .child(div().h(px(1.)).w_full().bg(sep_line))
            // --- Create tag row (real Input) ---
            .child({
                let this = this_entity.clone();
                div()
                    .pr(px(8.))
                    .h(px(26.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(26.))
                            .rounded(Radius::Md.scaled())
                            .bg(input_bg)
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .child(
                                Input::new(&self.create_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .w_full()
                                    .h(px(20.))
                                    .text_size(typography::font(type_role::BODY))
                                    .text_color(text_1),
                            )
                            // --- Handle Enter key on the parent div — uses the raw ---
                            // --- KeyDownEvent (like the clipboard note editor does), ---
                            // --- bypassing InputEvent::PressEnter subscriptions ---
                            // which cause stack corruption during action dispatch.
                            .on_key_down({
                                let this = this.clone();
                                move |ev: &KeyDownEvent, window, cx| {
                                    if ev.keystroke.key.as_str() == "enter" {
                                        cx.stop_propagation();
                                        this.update(cx, |panel, cx| {
                                            panel.create_tag_from_input(window, cx);
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    )
                    .child({
                        let this = this.clone();
                        widgets::btn_small(
                            &p,
                            "tag-filter-add",
                            I18nKey::TagFilterAdd.text(),
                            None,
                            widgets::Tone::Plain,
                        )
                        .on_click(move |_, window, cx| {
                            this.update(cx, |panel, cx| {
                                panel.create_tag_from_input(window, cx);
                                cx.notify();
                            });
                        })
                    })
            })
            .child(div().h(px(1.)).w_full().bg(sep_line))
            // --- Tag list ---
            .child({
                let scroll = self.scroll.clone();
                div()
                    .id("tag-filter-scroll")
                    .relative()
                    .flex()
                    .flex_col()
                    .size_full()
                    .max_h(px(200.))
                    .child(
                        div()
                            .id("tag-filter-scroll-area")
                            .flex()
                            .flex_col()
                            .size_full()
                            .track_scroll(scroll.handle())
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .flex_1()
                                    .pr(px(8.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .children(rows.into_iter().map(|row| {
                                        let this = this_entity.clone();
                                        div().flex().flex_row().gap(px(4.)).children(
                                            row.into_iter().map(|(tag, checked, pinned)| {
                                                let tag_id = tag.id;
                                                let tag_name_for_delete = tag.name.clone();
                                                let tag_name_edit = tag.name.clone();
                                                let tag_color_hex = tag.color.clone();
                                                let this = this.clone();
                                                let preview_tag = tag.clone();
                                                let preview_palette = p;
                                                let drag = ReorderDrag::new(
                                                    ReorderKey::Tag(tag_id),
                                                    move || {
                                                        filter_tag_cell(
                                                            &preview_tag,
                                                            checked,
                                                            pinned,
                                                            &preview_palette,
                                                        )
                                                        .child(small_btn_visual(
                                                            "EditIcon",
                                                            Rgba::from(preview_palette.ink_3),
                                                        ))
                                                        .child(small_btn_visual(
                                                            "DeleteIcon",
                                                            Rgba::from(preview_palette.ink_3),
                                                        ))
                                                        .into_any_element()
                                                    },
                                                );
                                                let row =
                                                    filter_tag_cell(&tag, checked, pinned, &p)
                                                        .id(("filter-tag", tag_id as u64))
                                                        .cursor(CursorStyle::PointingHand)
                                                        .hover(move |style| {
                                                            if checked {
                                                                style.bg(active_bg)
                                                            } else {
                                                                style.bg(btn_hover)
                                                            }
                                                        })
                                                        .on_click({
                                                            let this = this.clone();
                                                            move |_ev, _window, cx| {
                                                                this.update(cx, |panel, cx| {
                                                                    panel.toggle_filter(tag_id, cx)
                                                                });
                                                            }
                                                        })
                                                        .child(small_btn(
                                                            SharedString::from(format!(
                                                                "tag-cell:{tag_id}:edit"
                                                            )),
                                                            "EditIcon",
                                                            text_3,
                                                            false,
                                                            btn_hover,
                                                            danger_hover_bg,
                                                            {
                                                                let this = this.clone();
                                                                move |w, cx| {
                                                                    // --- Set edit name input value before updating state ---
                                                                    let input = this
                                                                        .read(cx)
                                                                        .edit_name_input
                                                                        .clone();
                                                                    input.update(
                                                                        cx,
                                                                        |input, cx| {
                                                                            input.set_value(
                                                                                &tag_name_edit,
                                                                                w,
                                                                                cx,
                                                                            );
                                                                        },
                                                                    );
                                                                    this.update(cx, |panel, cx| {
                                                                        panel.state.update(
                                                                            cx,
                                                                            |s, _cx| {
                                                                                s.start_edit_tag(
                                                                                    tag_id,
                                                                                    &tag_name_edit,
                                                                                    &tag_color_hex,
                                                                                );
                                                                            },
                                                                        );
                                                                        panel.last_edit_tag_id =
                                                                            tag_id;
                                                                        cx.notify();
                                                                    });
                                                                }
                                                            },
                                                        ))
                                                        .child(small_btn(
                                                            SharedString::from(format!(
                                                                "tag-cell:{tag_id}:delete"
                                                            )),
                                                            "DeleteIcon",
                                                            text_3,
                                                            true,
                                                            btn_hover,
                                                            danger_hover_bg,
                                                            {
                                                                let this = this.clone();
                                                                let target = TagDeleteTarget {
                                                                    id: tag_id,
                                                                    name: tag_name_for_delete,
                                                                };
                                                                move |_w, cx| {
                                                                    this.update(cx, |_panel, cx| {
                                                                        cx.emit(
                                                                TagFilterEvent::RequestDeleteTag(
                                                                    target.clone(),
                                                                ),
                                                            );
                                                                        cx.notify();
                                                                    })
                                                                }
                                                            },
                                                        ));
                                                drop_target(
                                                    drag_source(row, drag),
                                                    ReorderKey::Tag(tag_id),
                                                    true,
                                                    &p,
                                                    move |source, after, _, cx| {
                                                        let ReorderKey::Tag(source) = source;
                                                        this.update(cx, |panel, cx| {
                                                            panel.state.update(cx, |state, cx| {
                                                                state.reorder_tag(
                                                                    source, tag_id, after,
                                                                );
                                                                cx.notify();
                                                            });
                                                            cx.notify();
                                                        });
                                                    },
                                                )
                                            }),
                                        )
                                    })),
                            ),
                    )
                    .child(scroll.viewport_hook())
                    .scrollbar(scroll.handle(), ScrollbarAxis::Vertical)
            })
            .when(rows_is_empty, |el| {
                el.child(
                    div()
                        .py(px(8.))
                        .px(px(6.))
                        .text_size(typography::font(type_role::BODY))
                        .text_color(text_3)
                        .child(I18nKey::TagFilterNoTags.text()),
                )
            })
    }
}

fn filter_tag_cell(tag: &TagInfo, checked: bool, pinned: bool, p: &Palette) -> Div {
    let active_bg = Rgba::from(p.accent.opacity(0.13));
    let tag_color = parse_hex_to_rgba(&tag.color, Rgba::from(p.accent));
    let accent = Rgba::from(p.accent);
    let text_1 = Rgba::from(p.ink);
    let tag_name = tag.name.clone();
    div()
        .w(px(140.))
        .h(Ctrl::Btn.scaled())
        .rounded(Radius::Md.scaled())
        .bg(if checked {
            active_bg
        } else {
            Rgba::from(p.background.opacity(0.))
        })
        .flex()
        .flex_row()
        .items_center()
        .gap(Space::Xs.scaled())
        .px(Space::Sm.scaled())
        .child(if pinned {
            // Pin icon replacing color dot for pinned tags
            div()
                .w(px(10.))
                .h(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::icon("PinIcon", px(10.), Hsla::from(tag_color)))
        } else {
            div()
                .w(px(10.))
                .h(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(tag_color))
        })
        .child(
            div()
                .flex_1()
                .text_size(typography::font(type_role::BODY))
                .font_weight(if checked {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::default()
                })
                .text_color(if checked { accent } else { text_1 })
                .truncate()
                .child(tag_name),
        )
}

// --- TagEditPanel render helper ---

pub fn render_edit_panel(
    name_input: &Entity<InputState>,
    color: &str,
    p: Palette,
    scale: f32,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    on_color: impl Fn(&mut Window, &mut App, String) + 'static,
    on_save: impl Fn(&mut Window, &mut App, String, String) + 'static,
) -> impl IntoElement {
    let text_1 = Rgba::from(p.ink);
    let text_2 = Rgba::from(p.ink_2);
    let surface = Rgba::from(p.flyout);
    let input_bg = Rgba::from(p.on_card("control"));
    let sep_line = Rgba::from(p.stroke);
    let panel_border = Rgba::from(p.stroke_strong);
    let current_color = parse_hex_to_rgba(color, Rgba::from(p.accent));
    let presets: Vec<Rgba> = tag_preset_colors()
        .iter()
        .map(|(_, hex)| parse_hex_to_rgba(hex, Rgba::from(p.accent)))
        .collect();
    let color_rows: Vec<Vec<Rgba>> = presets.chunks(6).map(|r| r.to_vec()).collect();
    let on_cancel = Rc::new(on_cancel);
    let on_color = Rc::new(on_color);
    let on_save = Rc::new(on_save);

    div()
        .w(px(260. * scale))
        .bg(surface)
        .border_color(panel_border)
        .border_1()
        .rounded(Radius::Lg.scaled())
        .occlude()
        .p(Space::Lg.scaled())
        .flex()
        .flex_col()
        .gap(Space::Sm.scaled())
        .child(
            div()
                .text_size(typography::font(type_role::BASE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_1)
                .child(I18nKey::TagEditTitle.text()),
        )
        .child(div().h(px(1.)).w_full().bg(sep_line))
        .child(
            div()
                .text_size(typography::font(type_role::BODY))
                .text_color(text_2)
                .child(I18nKey::TagNameLabel.text()),
        )
        .child(
            div()
                .h(px(26.))
                .rounded(Radius::Md.scaled())
                .bg(input_bg)
                .px(px(8.))
                .flex()
                .items_center()
                .child(
                    Input::new(name_input)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .w_full()
                        .h(px(20.))
                        .px(px(0.))
                        .text_size(typography::font(type_role::BODY))
                        .text_color(text_1),
                ),
        )
        .child(div().h(px(1.)).w_full().bg(sep_line))
        .child(
            div()
                .text_size(typography::font(type_role::BODY))
                .text_color(text_2)
                .child(I18nKey::TagColor.text()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(color_rows.into_iter().map(|row| {
                    let on_color = on_color.clone();
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .children(row.into_iter().map(move |c| {
                            let on_color = on_color.clone();
                            let r = (c.r * 255.0) as u8;
                            let g = (c.g * 255.0) as u8;
                            let b = (c.b * 255.0) as u8;
                            let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);
                            div()
                                .w(px(36.))
                                .h(px(24.))
                                .rounded(px(4.))
                                .bg(c)
                                .border(if c == current_color { px(2.) } else { px(0.) })
                                .border_color(text_1)
                                .cursor(CursorStyle::PointingHand)
                                .occlude()
                                .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                                    on_color(window, cx, hex.clone());
                                })
                        }))
                })),
        )
        .child(div().h(px(1.)).w_full().bg(sep_line))
        .child({
            let r = (current_color.r * 255.0) as u8;
            let g = (current_color.g * 255.0) as u8;
            let b = (current_color.b * 255.0) as u8;
            let hex_save = format!("#{:02X}{:02X}{:02X}", r, g, b);
            div()
                .h(px(26.))
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .child(
                    widgets::btn_small(
                        &p,
                        "tag-editor-cancel",
                        I18nKey::BtnCancel.text(),
                        None,
                        widgets::Tone::Plain,
                    )
                    .on_click({
                        let on_cancel = on_cancel.clone();
                        move |_, window, cx| on_cancel(window, cx)
                    }),
                )
                .child(
                    widgets::btn_small(
                        &p,
                        "tag-editor-save",
                        I18nKey::BackendSave.text(),
                        None,
                        widgets::Tone::Accent,
                    )
                    .on_click({
                        let on_save = on_save.clone();
                        let name_input = name_input.clone();
                        let hex = hex_save.clone();
                        move |_, window, cx| {
                            let name = name_input.read(cx).value().to_string();
                            on_save(window, cx, name, hex.clone())
                        }
                    }),
                )
        })
}

// --- Helpers ---

fn icon_btn(
    control: &'static str,
    icon: &'static str,
    color: Rgba,
    hover_bg: Rgba,
    tooltip: Option<&'static str>,
    handler: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(control)
        .role(Role::Button)
        .tab_stop(true)
        .relative()
        .w(Ctrl::Chip.scaled())
        .h(Ctrl::Chip.scaled())
        .rounded(Radius::Md.scaled())
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(hover_bg))
        .when_some(tooltip, |button, tip| {
            button
                .aria_label(tip)
                .tooltip(move |window, cx| widgets::tooltip(tip, window, cx))
        })
        .child(widgets::icon_moving(
            control,
            icon,
            typography::font(type_role::BODY),
            Hsla::from(color),
        ))
        .child(widgets::icon_probe(control, CursorStyle::PointingHand))
        .on_mouse_down(MouseButton::Left, move |_e, w, cx| {
            cx.stop_propagation();
            handler(w, cx);
        })
}

fn small_btn(
    control: SharedString,
    icon: &'static str,
    color: Rgba,
    danger: bool,
    hover_bg: Rgba,
    danger_hover_bg: Rgba,
    handler: impl Fn(&mut Window, &mut App) + 'static,
) -> Div {
    div()
        .relative()
        .w(px(18.))
        .h(px(18.))
        .rounded(Radius::Sm.scaled())
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(if danger { danger_hover_bg } else { hover_bg }))
        .child(widgets::icon_moving(
            control.clone(),
            icon,
            typography::font(type_role::BODY),
            Hsla::from(color),
        ))
        .child(widgets::icon_probe(control, CursorStyle::PointingHand))
        .on_mouse_down(MouseButton::Left, move |_e, w, cx| {
            cx.stop_propagation();
            handler(w, cx);
        })
}

fn small_btn_visual(icon: &'static str, color: Rgba) -> Div {
    div()
        .w(px(18.))
        .h(px(18.))
        .rounded(Radius::Sm.scaled())
        .flex()
        .items_center()
        .justify_center()
        .child(widgets::icon(
            icon,
            typography::font(type_role::BODY),
            Hsla::from(color),
        ))
}

fn parse_hex_to_rgba(hex: &str, fallback: Rgba) -> Rgba {
    let s = hex.strip_prefix('#').unwrap_or(hex);
    if s.len() == 6 {
        if let Ok(val) = u32::from_str_radix(s, 16) {
            return rgb(val);
        }
    }
    fallback
}
