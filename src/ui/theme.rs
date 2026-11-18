//! 产品主题：`ClippiTheme` 是页面层仍在使用的旧结构，颜色已全部改由
//! `design::tokens`（`assets/theme.json`）派生。逐屏改写（M2+）期间两者
//! 并存：新代码走 `design::Palette`，旧代码继续读这里的字段。

use gpui::{Hsla, Rgba, WindowAppearance};

use super::design::{motion, tokens};

/// Semantic color palette for the Clippi UI.
/// Colors match the original Slint design pixel-for-pixel.
#[derive(Debug, Clone)]
pub struct ClippiTheme {
    // --- Window chrome ---
    pub bg: Rgba,
    pub surface: Rgba,
    pub surface_press: Rgba,

    // --- Text ---
    pub text_1: Rgba,
    pub text_2: Rgba,
    pub text_3: Rgba,

    // --- Accent ---
    pub accent: Rgba,
    pub accent_soft: Rgba,

    // --- Semantic ---
    pub fav_color: Rgba,
    pub danger: Rgba,
    pub divider: Rgba,
    pub btn_hover: Rgba,
    /// Transfer-station pinned state accent (blue), also used by the pin
    /// toolbar button and the pinned card bar.
    pub transfer_pin_color: Rgba,

    // --- Panels (floating) ---
    pub panel_surface: Rgba,
    pub panel_sep_line: Rgba,

    // --- Toast ---
    pub toast_bg: Rgba,

    /// 当前是否为暗色模式（旧代码曾用窗口底色字面量判断，令牌换值后会失效）。
    pub dark: bool,
}

impl ClippiTheme {
    pub fn dark() -> Self {
        Self::from_tokens(true)
    }

    pub fn light() -> Self {
        Self::from_tokens(false)
    }

    /// 全部颜色来自 `assets/theme.json`；语义映射见
    /// `docs/ui-redesign/design.md` §2。半透明令牌在旧组件上实色化
    /// （`on_card`），逐屏改写时再换真半透明层。
    fn from_tokens(dark: bool) -> Self {
        let color = |key: &str| tokens::color(key, dark);
        let accent = color("accent");
        let bg = color("window-bg");
        let accent_soft = motion::mix(bg.into(), accent.into(), 0.15);
        Self {
            bg,
            surface: tokens::on_card("layer", dark),
            surface_press: tokens::on_card("press", dark),

            text_1: color("ink"),
            text_2: color("ink-2"),
            text_3: color("ink-3"),

            accent,
            accent_soft: accent_soft.into(),

            fav_color: color("fav"),
            danger: color("danger"),
            divider: color("stroke"),
            btn_hover: tokens::on_card("hover", dark),
            transfer_pin_color: color("pin"),

            panel_surface: Rgba {
                a: 1.,
                ..color("flyout")
            },
            panel_sep_line: color("stroke"),

            toast_bg: Rgba {
                a: 1.,
                ..color("tip")
            },

            dark,
        }
    }

    pub fn from_settings(
        settings: &crate::core::settings::AppSettings,
        appearance: Option<WindowAppearance>,
    ) -> Self {
        let mut theme = Self::from_setting(&settings.theme, appearance);
        if settings.theme_color == "system" {
            if let Some(color) = *SYSTEM_ACCENT.lock().expect("system accent cache poisoned") {
                theme.accent = color;
                let weight = if theme.dark { 0.16 } else { 0.12 };
                theme.accent_soft = Rgba {
                    r: theme.bg.r * (1.0 - weight) + color.r * weight,
                    g: theme.bg.g * (1.0 - weight) + color.g * weight,
                    b: theme.bg.b * (1.0 - weight) + color.b * weight,
                    a: 1.0,
                };
            }
        }
        theme
    }

    pub fn from_setting(theme: &str, appearance: Option<WindowAppearance>) -> Self {
        match theme {
            "light" => Self::light(),
            "dark" => Self::dark(),
            _ => match appearance {
                Some(WindowAppearance::Dark | WindowAppearance::VibrantDark) => Self::dark(),
                _ => Self::light(),
            },
        }
    }

    /// Accent color as a 13% opacity overlay (used for active filter button backgrounds).
    pub fn accent_overlay(&self) -> Rgba {
        Rgba {
            a: 0x22 as f32 / 255.0,
            ..self.accent
        }
    }

    /// Accent color used behind search hits. Stronger than the generic
    /// overlay so short highlighted terms remain visible in dense previews.
    pub fn accent_highlight(&self) -> Rgba {
        if self.dark {
            Rgba {
                a: 0.4,
                ..self.accent
            }
        } else {
            Rgba {
                a: 0x4d as f32 / 255.0,
                ..self.accent
            }
        }
    }

    pub fn accent_highlight_text(&self) -> Rgba {
        if self.dark {
            Hsla::white().into()
        } else {
            Hsla::black().into()
        }
    }
}

static SYSTEM_ACCENT: std::sync::Mutex<Option<Rgba>> = std::sync::Mutex::new(None);

/// Called on the GPUI main thread at startup and by the existing poll loop.
pub fn refresh_system_accent() -> bool {
    static LAST_READ: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST_READ.lock().expect("system accent timer poisoned");
    if last.is_some_and(|time| time.elapsed() < std::time::Duration::from_secs(1)) {
        return false;
    }
    *last = Some(std::time::Instant::now());
    let color = crate::platform::accent::read_system_accent();
    let mut cached = SYSTEM_ACCENT.lock().expect("system accent cache poisoned");
    let changed = *cached != color;
    *cached = color;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_color_old_settings_default_and_round_trip() {
        let settings = crate::core::settings::AppSettings::default();
        let mut value = serde_json::to_value(settings).unwrap();
        value.as_object_mut().unwrap().remove("theme_color");
        let mut loaded: crate::core::settings::AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.theme_color, "default");
        loaded.theme_color = "system".into();
        let saved = toml::to_string(&loaded).unwrap();
        let restored: crate::core::settings::AppSettings = toml::from_str(&saved).unwrap();
        assert_eq!(restored.theme_color, "system");
    }

    #[test]
    fn overlays_follow_accent_and_preserve_default_colors() {
        // 令牌经 Hsla 往返有 ±1 量化误差，这里锁语义而非逐位色值。
        let dark = ClippiTheme::dark();
        let overlay = dark.accent_overlay();
        assert!((overlay.a - 0x22 as f32 / 255.0).abs() < 1e-6);
        assert_eq!(
            (overlay.r, overlay.g, overlay.b),
            (dark.accent.r, dark.accent.g, dark.accent.b)
        );
        let light = ClippiTheme::light();
        assert!((light.accent_highlight().a - 0x4d as f32 / 255.0).abs() < 1e-6);
        for mut theme in [ClippiTheme::light(), ClippiTheme::dark()] {
            theme.accent = gpui::rgb(0x0067c0);
            let overlay = theme.accent_overlay();
            let highlight = theme.accent_highlight();
            assert_eq!(
                (overlay.r, overlay.g, overlay.b),
                (theme.accent.r, theme.accent.g, theme.accent.b)
            );
            assert_eq!(
                (highlight.r, highlight.g, highlight.b),
                (theme.accent.r, theme.accent.g, theme.accent.b)
            );
        }
    }
}
