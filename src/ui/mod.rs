//! UI module declarations and re-exports for Clippi GPUI components.

pub mod add_backend;
pub mod assets;
pub mod bottom_bar;
pub mod clipboard_card;
pub mod clipboard_list;
pub mod components;
pub mod context_menu;
pub mod design;
pub mod edit_panel;
pub mod emoji_panel;
pub mod filter_bar;
pub mod font;
pub mod hover_toolbar;
pub mod i18n;
pub mod quick_paste;
pub mod rich_preview;
pub mod root;
pub mod search_box;
pub mod search_highlight;
pub mod settings;
pub mod sidebar;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub mod sidebar_window;
pub mod tag_filter;
pub mod tag_picker;
pub mod theme;
pub mod titlebar;
pub mod transition;
pub mod type_filter_config;
// M2 控件工厂：逐屏替换（M3+）前尚无调用方，过渡期允许未引用。
#[allow(dead_code)]
pub mod widgets;
pub mod window_manager;
