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
pub mod frame_trace;
pub mod hover_toolbar;
pub mod i18n;
pub mod icon_motion;
pub mod motion_pref;
pub mod preview;
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
pub mod titlebar;
pub mod transition;
pub mod type_filter_config;
// 控件工厂仍在逐屏接入：尚未迁到页面的构造器允许暂未引用。
#[allow(dead_code)]
pub mod widgets;
pub mod window_manager;
