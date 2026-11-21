//! Individual action tabs extending above the clipboard card.

use std::cell::Cell;
use std::rc::Rc;

use crate::ui::font::sc;

pub type ToolbarBounds = Rc<Cell<Option<(i64, Bounds<Pixels>)>>>;

type ToolbarActionHandler = Rc<dyn Fn(&str, &mut gpui::Window, &mut gpui::App)>;
/// 「更多」按钮：把点击位置交给调用方，由它弹出备注/标签菜单。
pub type ToolbarMoreHandler = Rc<dyn Fn(Point<Pixels>, &mut gpui::Window, &mut gpui::App)>;

type ToolbarColorFn = Box<dyn Fn(bool) -> Rgba>;

/// 单个操作按钮：图标名、动作名、常态/悬停换色。
pub struct ToolbarButton {
    icon: &'static str,
    action: &'static str,
    color: ToolbarColorFn,
}

impl ToolbarButton {
    pub fn action(&self) -> &'static str {
        self.action
    }
}

/// 第 `index` 个按钮左下角在窗口坐标里的位置，用作弹出菜单的锚点。
pub fn button_anchor(toolbar: Bounds<Pixels>, index: usize) -> Point<Pixels> {
    point(
        toolbar.left() + sc(index as f32 * (BUTTON_SIZE + BUTTON_GAP)),
        toolbar.bottom(),
    )
}

/// 按钮边长与间距（设计像素），宽度估算与渲染共用。
const BUTTON_SIZE: f32 = 20.0;
const BUTTON_GAP: f32 = 2.0;
/// 图标字形大小。按钮本身不画底框，所以字形比可点区域小一圈。
const BUTTON_ICON_SIZE: f32 = 14.0;

/// 一排按钮的总宽度（设计像素）。
pub fn toolbar_width(buttons: &[ToolbarButton]) -> f32 {
    if buttons.is_empty() {
        return 0.;
    }
    buttons.len() as f32 * BUTTON_SIZE + buttons.len().saturating_sub(1) as f32 * BUTTON_GAP
}

use gpui::prelude::*;
use gpui::*;

use super::widgets;

use crate::core::i18n_keys::I18nKey;
use crate::core::types::{ContentType, RichData};

use super::design::motion;
use super::design::palette::Palette;

/// Properties that determine which toolbar buttons to show.
pub struct HoverToolbarProps {
    pub content_type: ContentType,
    pub meta_type: String,
    pub full_text: String,
    pub has_rich_content: bool,
    pub has_qr_code: bool,
    pub has_links: bool,
    pub is_favorite: bool,
    /// 置顶状态（本地偏好；中转站条目的置顶走 `transfer_pinned`）。
    pub is_pinned: bool,
    pub selected_count: usize,
    pub is_selected: bool,
    pub can_merge_selection: bool,
    pub is_transfer: bool,
    pub transfer_is_local: bool,
    /// True when the transfer entry is pinned (remote manifest state).
    pub transfer_pinned: bool,
}

/// Link entries already represent a URL; extraction is for text containing links.
pub fn item_has_extractable_links(item: &crate::core::types::ClipboardItem) -> bool {
    item.meta_type != "link"
        && matches!(
            item.content_type,
            ContentType::PlainText | ContentType::RichText
        )
        && RichData::from_json(&item.rich_data).has_links == Some(true)
}

impl HoverToolbarProps {
    /// Derive from a ClipboardItem with external selection context.
    pub fn from_item(
        item: &crate::core::types::ClipboardItem,
        selected_count: usize,
        is_selected: bool,
    ) -> Self {
        Self {
            is_transfer: item.id < 0 && item.meta_type == "transfer",
            transfer_is_local: crate::core::types::FileData::from_json(&item.file_data)
                .files
                .first()
                .is_some_and(|file| !file.path.is_empty()),
            transfer_pinned: item
                .tags
                .iter()
                .any(|tag| tag.uid == crate::core::transfer_types::TRANSFER_STATUS_PINNED_UID),
            content_type: item.content_type,
            meta_type: item.meta_type.clone(),
            full_text: item.full_text.clone(),
            has_rich_content: matches!(
                item.display_kind(),
                crate::core::types::DisplayKind::Html
                    | crate::core::types::DisplayKind::Markdown
                    | crate::core::types::DisplayKind::Rtf
            ),
            has_qr_code: {
                let rich = RichData::from_json(&item.rich_data);
                rich.qr_text.is_some()
            },
            has_links: item_has_extractable_links(item),
            is_favorite: item.is_favorite,
            is_pinned: item.pinned,
            selected_count,
            is_selected,
            can_merge_selection: false,
        }
    }

    pub fn can_merge_selection(mut self, value: bool) -> Self {
        self.can_merge_selection = value;
        self
    }
}

/// Which open action a single entry offers, if any.
///
/// Links open in the browser, paths and files reveal in the file manager, and
/// text (including markdown/HTML/color entries) opens in the system's default
/// text editor.
fn single_open_action(props: &HoverToolbarProps) -> Option<&'static str> {
    if props.meta_type == "link" {
        return Some("open_location");
    }
    if props.meta_type == "path" {
        // A path that does not exist on this machine has nothing to reveal.
        return (crate::core::types::path_is_native(&props.full_text)
            && crate::core::types::path_exists(&props.full_text))
        .then_some("open_location");
    }
    match props.content_type {
        ContentType::File => Some("open_location"),
        ContentType::Image => Some("open_image"),
        ContentType::PlainText | ContentType::RichText => {
            // Secrets are masked in the UI on purpose — never spill one into a
            // plain-text file on disk.
            (props.meta_type != "secret").then_some("open_text")
        }
    }
}

#[derive(IntoElement)]
pub struct HoverToolbar {
    props: HoverToolbarProps,
    /// 预先构建好的按钮，避免宽度估算与渲染各算一次。
    buttons: Option<Vec<ToolbarButton>>,
    on_action: Option<ToolbarActionHandler>,
    on_more: Option<ToolbarMoreHandler>,
    bounds_tracker: Option<(i64, ToolbarBounds)>,
}

impl HoverToolbar {
    pub fn new(props: HoverToolbarProps) -> Self {
        Self {
            props,
            buttons: None,
            on_action: None,
            on_more: None,
            bounds_tracker: None,
        }
    }

    pub fn buttons(mut self, buttons: Vec<ToolbarButton>) -> Self {
        self.buttons = Some(buttons);
        self
    }

    pub fn track_bounds(mut self, item_id: i64, tracker: ToolbarBounds) -> Self {
        self.bounds_tracker = Some((item_id, tracker));
        self
    }

    pub fn on_action(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }

    /// 「更多」按钮的点击回调，参数是点击位置（窗口坐标）。
    pub fn on_more(
        mut self,
        handler: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_more = Some(Rc::new(handler));
        self
    }
}

/// 按卡片状态生成一排操作按钮。卡片先调用它算宽度，再把同一份结果交给
/// [`HoverToolbar`]，避免宽度估算与渲染各算一次。
pub fn buttons_for(props: &HoverToolbarProps, p: &Palette) -> Vec<ToolbarButton> {
    // Filled glyphs look heavier than body text at the same color. Soften
    // light-theme icons without losing the dark-theme contrast boost.
    let accent: Rgba = p.accent.into();
    let icon_color: Rgba = if p.dark {
        p.ink.into()
    } else {
        motion::mix(p.ink, p.ink_2, 0.5).into()
    };
    let danger: Rgba = p.danger.into();
    let fav_color: Rgba = p.fav.into();
    let pin_color: Rgba = p.pin.into();

    let is_single = props.selected_count <= 1;
    let is_batch = props.selected_count > 1 && props.is_selected;

    let mut buttons: Vec<ToolbarButton> = Vec::new();
    let mut push = |icon: &'static str, action: &'static str, color: ToolbarColorFn| {
        buttons.push(ToolbarButton {
            icon,
            action,
            color,
        });
    };

    if is_single
        && props.has_links
        && matches!(
            props.content_type,
            ContentType::PlainText | ContentType::RichText
        )
    {
        push(
            "LinkIcon",
            "extract_links",
            Box::new(move |hovered| if hovered { accent } else { icon_color }),
        );
    }
    if is_single && props.is_transfer {
        let transfer_pinned = props.transfer_pinned;
        if props.transfer_is_local {
            push(
                "OpenIcon",
                "open_transfer_location",
                Box::new(move |hovered| if hovered { accent } else { icon_color }),
            );
        } else {
            push(
                "DownloadIcon",
                "download_transfer",
                Box::new(move |hovered| if hovered { accent } else { icon_color }),
            );
        }
        // Pin/unpin — between open/download and delete.
        push(
            "PinIcon",
            "toggle_transfer_pin",
            Box::new(move |hovered| {
                if hovered || transfer_pinned {
                    pin_color
                } else {
                    icon_color
                }
            }),
        );
        push(
            "DeleteIcon",
            "delete_transfer",
            Box::new(move |hovered| if hovered { danger } else { icon_color }),
        );
    } else if is_single {
        // Copy
        push(
            "CopyIcon",
            "copy",
            Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
        );

        // --- Preview (single item) ---
        push(
            "PreviewIcon",
            "preview",
            Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
        );

        // --- Paste Plain Text (only when item has actual rich formatting) ---
        if props.has_rich_content {
            push(
                "TextIcon",
                "paste_plain",
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }

        // --- Paste as bitmap (image only) ---
        if props.content_type == ContentType::Image {
            push(
                "ImageIcon",
                "paste_image_bitmap",
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }

        // --- QR code (image with QR code) ---
        if props.content_type == ContentType::Image && props.has_qr_code {
            push(
                "QrCodeIcon",
                "qr_action",
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }

        // --- Edit (not image or file) ---
        if props.content_type != ContentType::Image && props.content_type != ContentType::File {
            push(
                "EditIcon",
                "edit",
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }

        // --- Open (one button for the whole family) ---
        if let Some(open_action) = single_open_action(props) {
            push(
                "OpenIcon",
                open_action,
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }

        // --- Pin (local view preference, sits next to favorite) ---
        // 置顶状态用主题色：与卡片左条、头部图钉同色，不再是独立的紫色。
        let is_pinned = props.is_pinned;
        push(
            "PinIcon",
            "toggle_pin",
            Box::new(move |hovered: bool| {
                if hovered || is_pinned {
                    accent
                } else {
                    icon_color
                }
            }),
        );

        // --- Favorite (icon changes based on state) ---
        let fav_icon = if props.is_favorite {
            "StarIcon"
        } else {
            "StarFilledIcon"
        };
        push(
            fav_icon,
            "toggle_favorite",
            Box::new(move |_hovered: bool| fav_color),
        );

        // --- More: 备注、标签等低频操作收进三点菜单 ---
        push(
            "MoreIcon",
            "more",
            Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
        );

        // --- Delete ---
        push(
            "DeleteIcon",
            "delete",
            Box::new(move |hovered: bool| if hovered { danger } else { icon_color }),
        );
    } else if is_batch && props.is_transfer {
        push(
            "DeleteIcon",
            "batch_delete_transfer",
            Box::new(move |hovered: bool| if hovered { danger } else { icon_color }),
        );
    } else if is_batch {
        // --- Batch paste ---
        push(
            "PasteIcon",
            "batch_paste",
            Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
        );
        if props.can_merge_selection {
            // --- Merge selected ---
            push(
                "FilesIcon",
                "merge_selected",
                Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
            );
        }
        // --- Batch tag ---
        push(
            "TagIcon",
            "batch_tag",
            Box::new(move |hovered: bool| if hovered { accent } else { icon_color }),
        );
        // --- Batch pin ---
        push(
            "PinIcon",
            "batch_pin",
            Box::new(
                move |hovered: bool| {
                    if hovered {
                        accent
                    } else {
                        icon_color
                    }
                },
            ),
        );
        // --- Batch favorite ---
        push(
            "StarIcon",
            "batch_favorite",
            Box::new(move |_hovered: bool| fav_color),
        );
        // --- Batch delete ---
        push(
            "DeleteIcon",
            "batch_delete",
            Box::new(move |hovered: bool| if hovered { danger } else { icon_color }),
        );
    }

    buttons
}

impl RenderOnce for HoverToolbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            props,
            buttons,
            on_action,
            on_more,
            bounds_tracker,
        } = self;

        let p = Palette::current(cx);
        let danger: Rgba = p.danger.into();
        let accent: Rgba = p.accent.into();

        let buttons = buttons.unwrap_or_else(|| buttons_for(&props, &p));
        if buttons.is_empty() {
            return div();
        }

        let is_favorite = props.is_favorite;
        let selected_count = props.selected_count;
        let meta_type = props.meta_type.clone();

        // Include the gaps in one hit area: the row underneath should not
        // receive clicks meant for a tab.
        let toolbar_width = toolbar_width(&buttons);
        let item_id = bounds_tracker.as_ref().map_or(0, |(id, _)| *id);
        div()
            .w(sc(toolbar_width))
            .h(sc(BUTTON_SIZE))
            .relative()
            .children(bounds_tracker.map(|(item_id, tracker)| {
                canvas(
                    move |bounds, _, _| tracker.set(Some((item_id, bounds))),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full()
            }))
            .flex()
            .flex_row()
            .items_center()
            .gap(sc(BUTTON_GAP))
            // The tabs overlap the preceding row. Own their hit area so mouse
            // movement cannot select that row or clear the active toolbar.
            // `block_mouse_except_scroll` (not `occlude`): GPUI's `occlude`
            // makes every hitbox behind this one report `should_handle_scroll()
            // == false`, which includes the list's scroll container — wheel
            // scrolling would die while the pointer is on the tabs. This keeps
            // the hover/click isolation and lets wheel deltas through, so the
            // list keeps scrolling under a stationary pointer.
            .block_mouse_except_scroll()
            .on_mouse_move(|_event, _window, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .on_mouse_down(MouseButton::Right, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .on_mouse_up(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .on_mouse_up(MouseButton::Right, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .children(buttons.into_iter().map(move |button| {
                let ToolbarButton {
                    icon,
                    action,
                    color: color_fn,
                } = button;
                let on_action = on_action.clone();
                let on_more = on_more.clone();
                let tooltip = match action {
                    "more" => I18nKey::CardMoreActions.text().to_string(),
                    "copy" => I18nKey::CtxCopy.text().to_string(),
                    "preview" => I18nKey::CtxPreview.text().to_string(),
                    "extract_links" => I18nKey::ExtractLinks.text().to_string(),
                    "paste_plain" => I18nKey::CtxPastePlain.text().to_string(),
                    "paste_image_bitmap" => I18nKey::CtxPasteImageBitmap.text().to_string(),
                    "open_image" => I18nKey::CtxOpenImage.text().to_string(),
                    "open_text" => I18nKey::CtxOpenText.text().to_string(),
                    "qr_action" => I18nKey::CtxDetectQr.text().to_string(),
                    "open_location" if meta_type == "link" => {
                        I18nKey::CtxOpenLink.text().to_string()
                    }
                    "open_location" => I18nKey::CtxOpenFolder.text().to_string(),
                    "edit" => I18nKey::CtxEdit.text().to_string(),
                    "edit_note" => I18nKey::EditNote.text().to_string(),
                    "show_tag_picker" => I18nKey::CtxTag.text().to_string(),
                    "toggle_favorite" if is_favorite => I18nKey::CtxUnfav.text().to_string(),
                    "toggle_favorite" => I18nKey::CtxFav.text().to_string(),
                    "delete" => I18nKey::CtxDelete.text().to_string(),
                    "batch_paste" => I18nKey::CtxBatchPasteN.fmt(&[&selected_count.to_string()]),
                    "merge_selected" => I18nKey::CtxMergeSelected.text().to_string(),
                    "batch_tag" => I18nKey::CtxBatchTag.text().to_string(),
                    "batch_favorite" => I18nKey::CtxBatchFav.text().to_string(),
                    "batch_delete" => I18nKey::CtxBatchDelete.text().to_string(),
                    "open_transfer_location" => I18nKey::CtxOpenFolder.text().to_string(),
                    "download_transfer" => I18nKey::DownloadTransfer.text().to_string(),
                    "toggle_transfer_pin" if props.transfer_pinned => {
                        I18nKey::TransferUnpin.text().to_string()
                    }
                    "toggle_transfer_pin" => I18nKey::TransferPin.text().to_string(),
                    "delete_transfer" => I18nKey::RemoveFromTransfer.text().to_string(),
                    "batch_delete_transfer" => {
                        I18nKey::RemoveSelectedFromTransfer.text().to_string()
                    }
                    _ => action.to_string(),
                };
                let is_delete = matches!(
                    action,
                    "delete" | "batch_delete" | "delete_transfer" | "batch_delete_transfer"
                );
                let action_id = action;
                let action = action.to_string();
                let icon = icon.to_string();
                let control: SharedString =
                    format!("toolbar-button-hover-{item_id}-{action_id}").into();
                let hovered = window.use_keyed_state(control.clone(), cx, |_, _| false);
                let motion_control = control.clone();
                let is_hovered = *hovered.read(cx);

                div()
                    .id(action_id)
                    .role(Role::Button)
                    .tab_stop(true)
                    .aria_label(tooltip.clone())
                    .w(sc(BUTTON_SIZE))
                    // 只有图标本身：不画底框、不画描边，悬停叠一层墨色遮罩并换色。
                    .h(sc(BUTTON_SIZE))
                    .flex_shrink_0()
                    .rounded(super::design::size::Radius::Md.scaled())
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(p.ink.opacity(0.06)))
                    .on_hover(move |value, _window, cx| {
                        if *value && !cx.reduce_motion() {
                            super::icon_motion::trigger(&motion_control);
                        }
                        hovered.update(cx, |hovered, cx| {
                            *hovered = *value;
                            cx.notify();
                        });
                    })
                    .tooltip(move |window, cx| widgets::tooltip(tooltip.clone(), window, cx))
                    .child({
                        let icon = icon.clone();
                        let color = if is_delete {
                            danger
                        } else if is_hovered {
                            accent
                        } else {
                            color_fn(false)
                        };
                        widgets::icon_moving(control, icon, sc(BUTTON_ICON_SIZE), Hsla::from(color))
                    })
                    .on_mouse_down(MouseButton::Left, {
                        let action = action.clone();
                        let is_more = action_id == "more";
                        move |ev, _window, cx| {
                            cx.stop_propagation();
                            if is_more {
                                if let Some(ref handler) = on_more {
                                    handler(ev.position, _window, cx);
                                }
                                return;
                            }
                            if let Some(ref handler) = on_action {
                                handler(&action, _window, cx);
                            }
                        }
                    })
            }))
    }
}

#[cfg(test)]
mod open_action_tests {
    use super::{item_has_extractable_links, single_open_action, HoverToolbarProps};
    use crate::core::types::{ClipboardItem, ContentType, RichData};

    fn props_for(item: &ClipboardItem) -> HoverToolbarProps {
        HoverToolbarProps::from_item(item, 1, false)
    }

    fn text_item(id: i64, meta_type: &str) -> ClipboardItem {
        let mut item = ClipboardItem::new_text(id, "content", ContentType::PlainText, None, None);
        item.meta_type = meta_type.to_string();
        item
    }

    #[test]
    fn extraction_is_offered_for_text_containing_links_but_not_link_entries() {
        let mut item = ClipboardItem::new_text(
            1,
            "https://www.bilibili.com/video/BV1s6xB6SE52?t=2114.5",
            ContentType::PlainText,
            None,
            Some(&RichData {
                has_links: Some(true),
                ..Default::default()
            }),
        );
        item.meta_type = "link".into();
        assert!(!item_has_extractable_links(&item));
        assert!(!props_for(&item).has_links);

        item.meta_type.clear();
        assert!(item_has_extractable_links(&item));
        assert!(props_for(&item).has_links);
        item.content_type = ContentType::RichText;
        assert!(item_has_extractable_links(&item));
        item.content_type = ContentType::File;
        assert!(!item_has_extractable_links(&item));
    }

    #[test]
    fn each_entry_type_routes_to_its_open_action() {
        let link = text_item(1, "link");
        assert_eq!(single_open_action(&props_for(&link)), Some("open_location"));

        let mut file = text_item(2, "");
        file.content_type = ContentType::File;
        assert_eq!(single_open_action(&props_for(&file)), Some("open_location"));

        let image = ClipboardItem::new_image(3, "", 0, 4, 4, None);
        assert_eq!(single_open_action(&props_for(&image)), Some("open_image"));

        let plain = text_item(4, "");
        assert_eq!(single_open_action(&props_for(&plain)), Some("open_text"));

        // Markdown and HTML are text too — they open as source.
        let markdown = text_item(5, "markdown");
        assert_eq!(single_open_action(&props_for(&markdown)), Some("open_text"));
        let html = ClipboardItem::new_text(
            6,
            "hello",
            ContentType::RichText,
            None,
            Some(&RichData {
                html: Some("<p>hello</p>".into()),
                ..Default::default()
            }),
        );
        assert_eq!(single_open_action(&props_for(&html)), Some("open_text"));
    }

    /// Text that is masked in the UI never gets an open button: it would be
    /// written to a plain-text file on disk.
    #[test]
    fn secrets_are_never_opened_in_an_editor() {
        let secret = text_item(7, "secret");
        assert_eq!(single_open_action(&props_for(&secret)), None);
    }

    /// A path entry that is gone from this machine has nothing to reveal.
    #[test]
    fn a_missing_path_has_no_open_action() {
        let mut missing = ClipboardItem::new_text(
            8,
            &std::env::temp_dir()
                .join("clippi-missing-open-target-9a1f")
                .to_string_lossy(),
            ContentType::PlainText,
            None,
            None,
        );
        missing.meta_type = "path".to_string();
        assert_eq!(single_open_action(&props_for(&missing)), None);

        let mut existing = ClipboardItem::new_text(
            9,
            &std::env::temp_dir().to_string_lossy(),
            ContentType::PlainText,
            None,
            None,
        );
        existing.meta_type = "path".to_string();
        assert_eq!(
            single_open_action(&props_for(&existing)),
            Some("open_location")
        );
    }
}
