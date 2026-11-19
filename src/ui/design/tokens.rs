//! `assets/theme.json` 的解析与读取：全仓唯一的颜色/尺寸/字体/动效来源。
//!
//! 结构对齐 Argus `frontend/theme.rs`。JSON 只解析一次（`OnceLock`），
//! 之后每次取色都是一次 map 查找 + 字符串解析；高频路径请走
//! `design::palette` 的缓存，而不是逐帧调用本模块。

use gpui::{px, relative, DefiniteLength, Hsla, Pixels, Rgba};

/// 解析后的令牌表（只读，进程内唯一）。
pub fn design() -> &'static serde_json::Value {
    static DESIGN: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    DESIGN.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/theme.json")).expect("theme.json")
    })
}

/// 一个颜色令牌：先找当前模式，缺失时回退亮色表（与 Argus 一致）。
pub fn token(name: &str, dark: bool) -> Hsla {
    let design = design();
    let mode = if dark { "dark" } else { "light" };
    parse_color(
        design[mode][name]
            .as_str()
            .or_else(|| design["light"][name].as_str())
            .unwrap_or_else(|| panic!("theme token `{name}` missing")),
    )
}

/// 一个布局令牌（`layout` 段），单位像素。
pub fn dimension(key: &str) -> Pixels {
    px(design()["layout"][key]
        .as_f64()
        .unwrap_or_else(|| panic!("layout token `{key}` missing")) as f32)
}

/// 基准字号（`fontSize`）：全部版式尺寸都是它的倍数。
pub fn base_font_size() -> f32 {
    design()["fontSize"]
        .as_f64()
        .expect("theme fontSize")
        .round() as f32
}

/// 全局继承行高（`lineHeight` 倍基准字号）。GPUI 默认约 1.618 会把换行
/// 文本和依赖行高的盒子撑得比设计稿高。
pub fn line_height() -> DefiniteLength {
    static LINE_HEIGHT: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    relative(
        *LINE_HEIGHT
            .get_or_init(|| design()["lineHeight"].as_f64().expect("theme lineHeight") as f32),
    )
}

/// 一个动效时长令牌（`"167ms"`），返回毫秒。
pub fn duration_ms(name: &str) -> u64 {
    design()["motion"][name]
        .as_str()
        .unwrap_or_else(|| panic!("motion token `{name}` missing"))
        .trim_end_matches("ms")
        .parse()
        .unwrap_or_else(|_| panic!("motion token `{name}` is not milliseconds"))
}

/// 解析 `#rrggbb` 或 `rgb(r g b / a)`（CSS 语法；分量 0-255，alpha 0-1）。
pub fn parse_color(css: &str) -> Hsla {
    if let Some(hex) = css.strip_prefix('#') {
        return gpui::rgb(u32::from_str_radix(hex, 16).expect("theme hex color")).into();
    }
    let components = css
        .strip_prefix("rgb(")
        .and_then(|s| s.strip_suffix(')'))
        .expect("theme RGB color")
        .split_whitespace()
        .filter(|part| *part != "/")
        .map(|part| part.parse::<f32>().expect("theme color component"))
        .collect::<Vec<_>>();
    Rgba {
        r: components[0] / 255.,
        g: components[1] / 255.,
        b: components[2] / 255.,
        a: components.get(3).copied().unwrap_or(1.),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rgb_tokens() {
        let hex = parse_color("#2e8f66");
        let rgba = Rgba::from(hex);
        assert!((rgba.a - 1.).abs() < f32::EPSILON);

        let translucent = parse_color("rgb(0 0 0 / 0.08)");
        assert!((Rgba::from(translucent).a - 0.08).abs() < 1e-6);

        let no_alpha = parse_color("rgb(255 255 255)");
        assert!((Rgba::from(no_alpha).a - 1.).abs() < f32::EPSILON);
    }

    #[test]
    fn every_palette_token_exists_in_both_modes() {
        // Palette 的构建依赖这些键；缺失时在启动首帧就会 panic，测试提前暴露。
        let keys = [
            "window-bg",
            "ink",
            "ink-2",
            "ink-3",
            "layer",
            "layer-strong",
            "hover",
            "hover-strong",
            "press",
            "stroke",
            "stroke-strong",
            "control",
            "control-hover",
            "control-checked",
            "flyout",
            "scrim",
            "accent",
            "on-accent",
            "fav",
            "danger",
            "on-danger",
            "pin",
            "tag-bg",
            "tag-ink",
            "tip",
            "scroll-thumb",
            "scroll-thumb-hover",
        ];
        for name in keys {
            let _ = token(name, false);
            let _ = token(name, true);
        }
    }

    #[test]
    fn layout_and_motion_tokens_are_readable() {
        assert!(f32::from(dimension("unit")) > 0.);
        assert_eq!(duration_ms("t-fast"), 167);
        assert_eq!(duration_ms("t-slow"), 250);
        assert!(base_font_size() > 0.);
    }
}
