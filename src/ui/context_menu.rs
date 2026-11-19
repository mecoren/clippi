//! Context menu — right-click menu for clipboard items.
//!
//! --- Matches the original Slint ContextMenu.slint design: ---
//! --- - 164px width, 8px border-radius, 4px padding ---
//! --- - 30px item height, 5px border-radius per item ---
//! --- - Icon (13px Lucide) + label (13px) per row ---
//! --- - Separators: 3px gap + 1px line + 3px gap ---
//! --- - Single and batch mode variants with conditional items ---
//! --- - Position clamping to container bounds ---

use crate::ui::design::palette::Palette;
use crate::ui::design::size::Radius;
use crate::ui::font::fs;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::widgets;

use crate::core::i18n_keys::I18nKey;

type MenuActionHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type MenuDismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Context describing which menu items to show.
#[derive(Default)]
pub struct MenuItemContext {
    pub is_image: bool,
    pub is_file: bool,
    pub is_color: bool,
    pub is_rich_text: bool,
    /// true = current format is HEX → show "Paste as RGB"
    pub is_hex: bool,
    pub is_favorite: bool,
    pub is_link: bool,
    pub is_path: bool,
    /// Text entry (plain or rich) that has no dedicated open action of its own —
    /// it opens in the system's default text editor. Secrets are excluded: they
    /// are masked in the UI and must not be written to a file.
    pub opens_in_editor: bool,
    pub has_hotkey: bool,
    /// True if this file item has been uploaded to the transfer station.
    pub is_transfer: bool,
    /// True if the transfer station is enabled and has a usable backend.
    pub transfer_enabled: bool,
}

impl MenuItemContext {
    pub fn from_item(item: &crate::core::types::ClipboardItem) -> Self {
        use crate::core::types::{ContentType, DisplayKind};
        let is_color = item.meta_type == "color";
        // --- is_hex = true → show "Paste as RGB" (convert FROM hex) ---
        let is_hex = is_color && crate::core::color::is_hex_format(&item.full_text);
        // Only show "paste as plain text" when item has actual rich formatting
        let is_rich_text = matches!(
            item.display_kind(),
            DisplayKind::Html | DisplayKind::Markdown | DisplayKind::Rtf
        );
        let file_data = crate::core::types::FileData::from_json(&item.file_data);
        let is_transfer = item.content_type == ContentType::File
            && (file_data.is_transfer() || !file_data.remote_hash.is_empty());
        Self {
            is_image: item.content_type == ContentType::Image,
            is_file: item.content_type == ContentType::File,
            is_color,
            is_rich_text,
            is_hex,
            is_favorite: item.is_favorite,
            is_link: item.meta_type == "link",
            is_path: item.meta_type == "path"
                && crate::core::types::path_is_native(&item.full_text)
                && crate::core::types::path_exists(&item.full_text),
            opens_in_editor: matches!(
                item.content_type,
                ContentType::PlainText | ContentType::RichText
            ) && !matches!(item.meta_type.as_str(), "link" | "path" | "secret"),
            has_hotkey: !item.custom_hotkey.is_empty(),
            is_transfer,
            transfer_enabled: false,
        }
    }
}

#[derive(IntoElement)]
pub struct ContextMenu {
    items: Vec<RawMenuItem>,
    x: f32,
    y: f32,
    /// 菜单宽度；只有两条短文案的菜单可以收窄。
    width: f32,
    container_width: f32,
    container_height: f32,
    on_action: Option<MenuActionHandler>,
    on_dismiss: Option<MenuDismissHandler>,
}

/// Internal menu item descriptor.
struct RawMenuItem {
    label: String,
    action: String,
    icon: String,
    danger: bool,
    fav: bool,
    shortcut: Option<String>,
}

/// Platform-aware modifier key label for shortcut display.
fn ctrl_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "\u{2318}"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl"
    }
}

const SEPARATOR_LABEL: &str = "__sep__";

fn sc(label: &str) -> Option<String> {
    Some(label.into())
}

fn ctrl_shortcut(key: &str) -> Option<String> {
    Some(format!("{}+{}", ctrl_label(), key))
}
const MENU_WIDTH: f32 = 184.0;
/// 条目少且文案短的菜单（如卡片三点菜单）用这一档宽度。
const MENU_WIDTH_COMPACT: f32 = 136.0;
/// Generic open glyph, shared by every open action so one icon means "open"
/// whatever the entry type is.
const OPEN_ICON: &str = "OpenIcon";
/// 菜单左右内边距，条目宽度 = 菜单宽度 - 2 × 该值。
const MENU_PADDING: f32 = 4.0;
/// Height of a single menu item row.
const ITEM_HEIGHT: f32 = 30.0;
/// Height of a separator (3px gap + 1px line + 3px gap).
const SEPARATOR_HEIGHT: f32 = 7.0;
/// Vertical padding (4px top + 4px bottom).
const MENU_V_PADDING: f32 = 8.0;

impl ContextMenu {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: MENU_WIDTH,
            container_width: f32::MAX,
            container_height: f32::MAX,
            on_action: None,
            on_dismiss: None,
        }
    }

    /// Build a single-item context menu from item context.
    pub fn for_item(ctx: &MenuItemContext) -> Self {
        let mut items: Vec<RawMenuItem> = Vec::new();

        // Copy
        items.push(RawMenuItem {
            label: I18nKey::CtxCopy.text().into(),
            action: "copy".into(),
            icon: "CopyIcon".into(),
            danger: false,
            fav: false,
            shortcut: ctrl_shortcut("C"),
        });
        // --- Paste ---
        items.push(RawMenuItem {
            label: I18nKey::CtxPaste.text().into(),
            action: "paste".into(),
            icon: "PasteIcon".into(),
            danger: false,
            fav: false,
            shortcut: sc("Enter"),
        });
        // --- Paste Plain Text (rich text only) ---
        if ctx.is_rich_text {
            items.push(RawMenuItem {
                label: I18nKey::CtxPastePlain.text().into(),
                action: "paste_plain".into(),
                icon: "TextIcon".into(),
                danger: false,
                fav: false,
                shortcut: sc("Shift+Enter"),
            });
        }

        // Color conversion (only for color type)
        if ctx.is_color {
            let (label, action) = if ctx.is_hex {
                (I18nKey::CtxPasteAsRgb.text(), "paste_as_rgb")
            } else {
                (I18nKey::CtxPasteAsHex.text(), "paste_as_hex")
            };
            items.push(RawMenuItem {
                label: label.into(),
                action: action.into(),
                icon: "ColorIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }

        // --- Separator ---
        items.push(RawMenuItem {
            label: SEPARATOR_LABEL.into(),
            action: String::new(),
            icon: String::new(),
            danger: false,
            fav: false,
            shortcut: None,
        });

        // Edit (not for image/file)
        if !ctx.is_image && !ctx.is_file {
            items.push(RawMenuItem {
                label: I18nKey::CtxEdit.text().into(),
                action: "edit".into(),
                icon: "EditIcon".into(),
                danger: false,
                fav: false,
                shortcut: ctrl_shortcut("E"),
            });
        }

        // Note
        items.push(RawMenuItem {
            label: I18nKey::EditNote.text().into(),
            action: "edit_note".into(),
            icon: "NoteIcon".into(),
            danger: false,
            fav: false,
            shortcut: sc("F2"),
        });

        // Custom hotkey
        if ctx.has_hotkey {
            items.push(RawMenuItem {
                label: I18nKey::CtxChangeHotkey.text().into(),
                action: "set_hotkey".into(),
                icon: "KeyboardIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
            items.push(RawMenuItem {
                label: I18nKey::CtxRemoveHotkey.text().into(),
                action: "remove_hotkey".into(),
                icon: "CloseIcon".into(),
                danger: true,
                fav: false,
                shortcut: None,
            });
        } else {
            items.push(RawMenuItem {
                label: I18nKey::CtxSetHotkey.text().into(),
                action: "set_hotkey".into(),
                icon: "KeyboardIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }

        // --- Open original image (image only) ---
        if ctx.is_image {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenImage.text().into(),
                action: "open_image".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
            items.push(RawMenuItem {
                label: I18nKey::CtxPasteImagePath.text().into(),
                action: "paste_image_path".into(),
                icon: "TextIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
            items.push(RawMenuItem {
                label: I18nKey::CtxPasteImageBitmap.text().into(),
                action: "paste_image_bitmap".into(),
                icon: "ImageIcon".into(),
                danger: false,
                fav: false,
                shortcut: ctrl_shortcut("Enter"),
            });
            items.push(RawMenuItem {
                label: I18nKey::CtxPasteOcr.text().into(),
                action: "paste_ocr".into(),
                icon: "OcrIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
            items.push(RawMenuItem {
                label: I18nKey::CtxDetectQr.text().into(),
                action: "qr_detect".into(),
                icon: "QrCodeIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }

        // Tag
        items.push(RawMenuItem {
            label: I18nKey::CtxTag.text().into(),
            action: "show_tag_picker".into(),
            icon: "TagIcon".into(),
            danger: false,
            fav: false,
            shortcut: ctrl_shortcut("T"),
        });

        // --- Open in browser (link only) ---
        if ctx.is_link {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenLink.text().into(),
                action: "open_location".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
        }
        // --- Jump to directory (path only) ---
        if ctx.is_path {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenFolder.text().into(),
                action: "open_location".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
        }
        // --- Open folder (file only) ---
        if ctx.is_file {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenFolder.text().into(),
                action: "open_location".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
        }
        // --- Open in the default text editor (text only) ---
        if ctx.opens_in_editor {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenText.text().into(),
                action: "open_text".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
        }
        // --- Paste file path (file only) ---
        if ctx.is_file {
            items.push(RawMenuItem {
                label: I18nKey::CtxPasteFilePath.text().into(),
                action: "paste_file_path".into(),
                icon: "TextIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }
        // --- Upload to transfer station (file only, not already uploaded) ---
        if ctx.is_file && !ctx.is_transfer && ctx.transfer_enabled {
            items.push(RawMenuItem {
                label: I18nKey::UploadToTransfer.text().into(),
                action: "upload_to_transfer".into(),
                icon: "UploadIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }

        // --- Separator ---
        items.push(RawMenuItem {
            label: SEPARATOR_LABEL.into(),
            action: String::new(),
            icon: String::new(),
            danger: false,
            fav: false,
            shortcut: None,
        });

        // --- Favorite ---
        let (fav_label, fav_icon) = if ctx.is_favorite {
            (I18nKey::CtxUnfav.text(), "StarIcon")
        } else {
            (I18nKey::CtxFav.text(), "StarFilledIcon")
        };
        items.push(RawMenuItem {
            label: fav_label.into(),
            action: "toggle_favorite".into(),
            icon: fav_icon.into(),
            danger: false,
            fav: true,
            shortcut: ctrl_shortcut("D"),
        });

        // --- Delete ---
        items.push(RawMenuItem {
            label: I18nKey::CtxDelete.text().into(),
            action: "delete".into(),
            icon: "DeleteIcon".into(),
            danger: true,
            fav: false,
            shortcut: sc("Delete"),
        });

        Self::new().items(items)
    }

    /// Build a batch context menu.
    pub fn for_batch(selected_count: usize, can_merge: bool) -> Self {
        let mut items = vec![RawMenuItem {
            label: I18nKey::CtxBatchPasteN.fmt(&[&selected_count.to_string()]),
            action: "batch_paste".into(),
            icon: "PasteIcon".into(),
            danger: false,
            fav: false,
            shortcut: None,
        }];

        if can_merge {
            items.push(RawMenuItem {
                label: I18nKey::CtxMergeSelected.text().into(),
                action: "merge_selected".into(),
                icon: "FilesIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }

        items.extend([
            RawMenuItem {
                label: SEPARATOR_LABEL.into(),
                action: String::new(),
                icon: String::new(),
                danger: false,
                fav: false,
                shortcut: None,
            },
            RawMenuItem {
                label: I18nKey::CtxBatchTag.text().into(),
                action: "show_tag_picker".into(),
                icon: "TagIcon".into(),
                danger: false,
                fav: false,
                shortcut: ctrl_shortcut("T"),
            },
            RawMenuItem {
                label: SEPARATOR_LABEL.into(),
                action: String::new(),
                icon: String::new(),
                danger: false,
                fav: false,
                shortcut: None,
            },
            RawMenuItem {
                label: I18nKey::CtxBatchFav.text().into(),
                action: "batch_favorite".into(),
                icon: "StarIcon".into(),
                danger: false,
                fav: true,
                shortcut: ctrl_shortcut("D"),
            },
            RawMenuItem {
                label: I18nKey::CtxBatchDelete.text().into(),
                action: "batch_delete".into(),
                icon: "DeleteIcon".into(),
                danger: true,
                fav: false,
                shortcut: sc("Delete"),
            },
        ]);
        Self::new().items(items)
    }

    /// Build a batch context menu for transfer-station entries.
    pub fn for_transfer_batch() -> Self {
        Self::new().items(vec![RawMenuItem {
            label: I18nKey::RemoveSelectedFromTransfer.text().into(),
            action: "batch_delete_transfer".into(),
            icon: "DeleteIcon".into(),
            danger: true,
            fav: false,
            shortcut: sc("Delete"),
        }])
    }

    /// Build a context menu for a transfer station entry.
    /// Shows download (cloud-only), open location (local-only), pin/unpin,
    /// and delete.
    pub fn for_transfer_entry(is_local: bool, is_pinned: bool) -> Self {
        let mut items = Vec::new();
        if !is_local {
            items.push(RawMenuItem {
                label: I18nKey::DownloadTransfer.text().into(),
                action: "download_transfer".into(),
                icon: "DownloadIcon".into(),
                danger: false,
                fav: false,
                shortcut: None,
            });
        }
        if is_local {
            items.push(RawMenuItem {
                label: I18nKey::CtxOpenFolder.text().into(),
                action: "open_transfer_location".into(),
                icon: OPEN_ICON.into(),
                danger: false,
                fav: false,
                shortcut: sc("Space"),
            });
        }
        items.push(RawMenuItem {
            label: if is_pinned {
                I18nKey::TransferUnpin.text().into()
            } else {
                I18nKey::TransferPin.text().into()
            },
            action: "toggle_transfer_pin".into(),
            icon: "PinIcon".into(),
            danger: false,
            fav: false,
            shortcut: None,
        });
        // Separator
        items.push(RawMenuItem {
            label: SEPARATOR_LABEL.into(),
            action: String::new(),
            icon: String::new(),
            danger: false,
            fav: false,
            shortcut: None,
        });
        items.push(RawMenuItem {
            label: I18nKey::RemoveFromTransfer.text().into(),
            action: "delete_transfer".into(),
            icon: "DeleteIcon".into(),
            danger: true,
            fav: false,
            shortcut: sc("Delete"),
        });
        Self::new().items(items)
    }

    /// 「更多操作」菜单：卡片头部的三点按钮弹出，只放低频动作
    /// （备注、标签），常用动作仍留在悬浮工具条上。
    pub fn for_card_more() -> Self {
        // 只有两条短文案，按最窄一档显示，避免在卡片右侧留一大片空白。
        Self::new().width(MENU_WIDTH_COMPACT).items(vec![
            RawMenuItem {
                label: I18nKey::EditNote.text().into(),
                action: "edit_note".into(),
                icon: "NoteIcon".into(),
                danger: false,
                fav: false,
                shortcut: sc("F2"),
            },
            RawMenuItem {
                label: I18nKey::CtxTag.text().into(),
                action: "show_tag_picker".into(),
                icon: "TagIcon".into(),
                danger: false,
                fav: false,
                shortcut: ctrl_shortcut("T"),
            },
        ])
    }

    fn items(mut self, items: Vec<RawMenuItem>) -> Self {
        self.items = items;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn with_position(
        mut self,
        x: f32,
        y: f32,
        container_width: f32,
        container_height: f32,
    ) -> Self {
        self.x = x;
        self.y = y;
        self.container_width = container_width;
        self.container_height = container_height;
        self
    }

    pub fn on_action(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }

    /// Estimate the rendered height of this menu based on its items.
    pub fn estimated_height(&self) -> f32 {
        let mut h: f32 = MENU_V_PADDING;
        for item in &self.items {
            if item.label == SEPARATOR_LABEL {
                h += SEPARATOR_HEIGHT;
            } else {
                h += ITEM_HEIGHT;
            }
        }
        h
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        // --- Compute menu height BEFORE destructuring self ---
        let menu_h = self.estimated_height();

        let Self {
            items,
            x,
            y,
            width,
            container_width,
            container_height,
            on_action,
            on_dismiss,
        } = self;

        let p = Palette::current(_cx);
        let surface = p.flyout;
        let sep_line = p.stroke;
        // Floating menus need more contrast than buttons on the main surface.
        let item_hover = p.on_card("press");
        let accent = p.accent;
        let text_1 = p.ink;
        let text_2 = p.ink_2;
        let text_3 = p.ink_3;
        let danger = p.danger;
        let fav_color = p.fav;

        // --- Clamp position to container bounds with height awareness. ---
        // Flips the menu above the cursor if it would overflow the bottom edge.
        let menu_w = width;
        let item_w = menu_w - MENU_PADDING * 2.0;
        let clamped_x = x.clamp(4.0, (container_width - menu_w - 4.0).max(4.0));
        // Prefer below cursor; flip above if it overflows the bottom.
        let clamped_y = if y + menu_h + 4.0 <= container_height {
            // --- Fits below cursor — small 2px gap from click point ---
            y.clamp(4.0, container_height - menu_h - 4.0)
        } else {
            // --- Flip above cursor — 8px gap from click point ---
            (y - menu_h - 8.0).clamp(4.0, container_height - menu_h - 4.0)
        };

        div()
            .absolute()
            .left(px(clamped_x))
            .top(px(clamped_y))
            .w(px(menu_w))
            .rounded(px(8.))
            .bg(surface)
            .border(px(1.))
            .border_color(sep_line)
            // 菜单不投影：层级靠 `flyout` 面 + 描边（Argus 只给对话框阴影）。
            .shadow_none()
            .p(px(MENU_PADDING))
            .flex()
            .flex_col()
            .on_key_down({
                let on_dismiss = on_dismiss.clone();
                move |ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key.as_str() == "escape" {
                        cx.stop_propagation();
                        if let Some(ref dismiss) = on_dismiss {
                            dismiss(window, cx);
                        }
                    }
                }
            })
            .children(items.into_iter().map(|item| {
                let on_action = on_action.clone();
                let on_dismiss = on_dismiss.clone();

                // --- Render separator ---
                if item.label == SEPARATOR_LABEL {
                    return div()
                        .w(px(item_w))
                        .flex()
                        .flex_col()
                        .child(div().h(px(3.)))
                        .child(div().w_full().h(px(1.)).bg(sep_line))
                        .child(div().h(px(3.)))
                        .into_any_element();
                }

                let is_danger = item.danger;
                let is_fav = item.fav;
                let action = item.action.clone();
                let icon = item.icon.clone();
                let label = item.label.clone();
                let shortcut = item.shortcut.clone();

                let normal_icon = if is_fav {
                    fav_color
                } else if is_danger {
                    danger
                } else {
                    text_2
                };
                let normal_text = if is_fav {
                    fav_color
                } else if is_danger {
                    danger
                } else {
                    text_1
                };

                // --- Hover colors: fav stays fav, danger stays danger, otherwise accent ---
                let hover_icon = if is_fav {
                    fav_color
                } else if is_danger {
                    danger
                } else {
                    accent
                };
                let hover_text = if is_fav {
                    fav_color
                } else if is_danger {
                    danger
                } else {
                    accent
                };

                let control: SharedString = format!("context-menu-{action}").into();
                div()
                    .id(ElementId::Name(control.clone()))
                    .role(Role::MenuItem)
                    .aria_label(label.clone())
                    .relative()
                    .w(px(item_w))
                    .h(px(30.))
                    .rounded(Radius::Md.scaled())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(8.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(item_hover))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child({
                                let icon = icon.clone();
                                widgets::icon_moving_hover(
                                    control.clone(),
                                    icon,
                                    fs(13.),
                                    normal_icon,
                                    hover_icon,
                                )
                            })
                            .child({
                                let label = label.clone();
                                div()
                                    .text_size(fs(13.))
                                    .text_color(normal_text)
                                    .hover(move |style| style.text_color(hover_text))
                                    .child(label)
                            }),
                    )
                    .when(shortcut.is_some(), move |row| {
                        row.child(
                            div()
                                .text_size(fs(10.))
                                .text_color(text_3)
                                .child(shortcut.clone().unwrap()),
                        )
                    })
                    .child(widgets::icon_probe(control, CursorStyle::PointingHand))
                    .on_mouse_down(MouseButton::Left, {
                        let action = action.clone();
                        move |_ev, window, cx| {
                            if let Some(ref handler) = on_action {
                                handler(&action, window, cx);
                            }
                            if let Some(ref dismiss) = on_dismiss {
                                dismiss(window, cx);
                            }
                        }
                    })
                    .into_any_element()
            }))
    }
}
