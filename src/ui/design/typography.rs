//! 排版：字号档位、等宽数字、字体栈回退（对齐 Argus `theme.rs` 的
//! `font` / `fallbacks` / `installed_font` / `tabular`）。
//!
//! 全部尺寸经 [`super::super::font`] 的全局缩放系数，保留现有
//! 0.92–1.30 的字号设置能力。

use std::sync::{Arc, OnceLock};

use gpui::{App, FontFeatures, FontWeight, Pixels, SharedString};

use super::tokens::{base_font_size, design};

/// 名称/标题/导航的强调字重。600 在中文上会选到 YaHei UI Bold，故用 Medium。
pub const LABEL_WEIGHT: FontWeight = FontWeight::MEDIUM;

/// 字号档位（基准值，经全局缩放）。
pub mod type_role {
    /// 徽标、keycaps、时间戳单位
    pub const CAPTION: f32 = 10.;
    /// 卡片正文、菜单项、底栏
    pub const BODY: f32 = 12.;
    /// 设置行标题、输入框、按钮
    pub const BASE: f32 = 13.;
    /// 设置页标题、对话框标题
    pub const TITLE: f32 = 16.;
    /// 大数字、空状态主标题
    pub const DISPLAY: f32 = 26.;
}

/// 设计字号 → 实际像素（叠加用户字号缩放）。
#[inline]
pub fn font(size: f32) -> Pixels {
    super::super::font::fs(size)
}

/// 取 `layout` 里的字号令牌（随用户缩放）。
pub fn font_dimension(key: &str) -> Pixels {
    font(f32::from(super::tokens::dimension(key)))
}

/// 基准字号（13）随用户缩放后的值；rem 联动用。
pub fn scaled_base() -> Pixels {
    font(base_font_size())
}

/// 等宽数字特性：时间戳、大小、计数、Alt 序号在同一列里不跳动。
pub fn tabular() -> FontFeatures {
    static FEATURES: OnceLock<FontFeatures> = OnceLock::new();
    FEATURES
        .get_or_init(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
        .clone()
}

/// `fonts[kind]` 中第一个系统真实安装的族名；缺失回落系统 UI 字体。
///
/// 与 Argus 的 `installed_font` 等价，但沿用本项目 Windows 侧的
/// DirectWrite 检测（`platform::system_font`），macOS 交给 CoreText。
pub fn installed_family(kind: &str, _cx: &App) -> SharedString {
    static SANS: OnceLock<SharedString> = OnceLock::new();
    static MONO: OnceLock<SharedString> = OnceLock::new();
    static TOOLTIP: OnceLock<SharedString> = OnceLock::new();
    let cell = match kind {
        "mono" => &MONO,
        "tooltip" => &TOOLTIP,
        _ => &SANS,
    };
    cell.get_or_init(|| {
        let stack = design()["fonts"][kind]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str);
        #[cfg(target_os = "windows")]
        let installed = stack
            .into_iter()
            .find(|name| crate::platform::system_font::family_is_registered(name))
            .map(SharedString::from);
        #[cfg(not(target_os = "windows"))]
        let installed = stack.map(SharedString::from).next();
        installed.unwrap_or_else(super::super::font::system_family)
    })
    .clone()
}

/// 界面字体族之后的 CJK/符号回退链（修复英文系统把中文渲染成日文字体）。
pub fn fallbacks(cx: &App) -> gpui::FontFallbacks {
    static SANS: OnceLock<gpui::FontFallbacks> = OnceLock::new();
    SANS.get_or_init(|| {
        let family = installed_family("sans", cx);
        let chain = design()["fonts"]["sans"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .skip_while(|name| *name != family.as_ref())
            .skip(1)
            .chain(
                design()["fonts"]["fallback"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str),
            )
            .filter(|name| *name != "Microsoft YaHei UI")
            .map(str::to_owned)
            .collect();
        gpui::FontFallbacks::from_fonts(chain)
    })
    .clone()
}

/// 提示浮层字体（系统 tooltip 的族）。
pub fn tooltip_family(cx: &App) -> SharedString {
    installed_family("tooltip", cx)
}

/// 等宽族（地址、路径、代码）。
pub fn mono_family(cx: &App) -> SharedString {
    installed_family("mono", cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_roles_keep_hierarchy() {
        let roles = [
            type_role::CAPTION,
            type_role::BODY,
            type_role::BASE,
            type_role::TITLE,
            type_role::DISPLAY,
        ];
        assert!(roles.windows(2).all(|pair| pair[0] < pair[1]), "{roles:?}");
    }

    #[test]
    fn tabular_requests_equal_width_digits() {
        let features = tabular();
        assert_eq!(features.0.len(), 1);
        assert_eq!(&*features.0[0].0, "tnum");
    }
}
