//! 语义调色板：令牌 → 角色色，线程内缓存；`apply()` 把整表灌进
//! gpui-kit 的全局主题（输入框、滚动条、提示层随之统一）。
//!
//! 对齐 Argus `theme.rs:9-197`（Palette/缓存/`on_card`/`remainder`）。

use gpui::{px, App, Hsla, Rgba, Window};
use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};

use super::{motion, tokens};

/// 一次主题构建产出的语义色（页面层唯一取色入口）。
#[derive(Clone, Copy)]
pub struct Palette {
    // 文字三级
    pub ink: Hsla,
    pub ink_2: Hsla,
    pub ink_3: Hsla,
    // 表面
    pub background: Hsla,
    pub layer: Hsla,
    pub layer_strong: Hsla,
    pub flyout: Hsla,
    // 描边
    pub stroke: Hsla,
    pub stroke_strong: Hsla,
    // 交互叠加
    pub hover: Hsla,
    pub hover_strong: Hsla,
    pub press: Hsla,
    // 控件
    pub control_face: Hsla,
    pub control_hover: Hsla,
    pub control_checked: Hsla,
    pub control_edge: Hsla,
    // 强调与语义
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub fav: Hsla,
    pub danger: Hsla,
    pub on_danger: Hsla,
    /// 注意/警告（路径失效、同步异常），与收藏金、危险红区分。
    pub warn: Hsla,
    /// 成功/完成（传输完成、同步正常）。
    pub ok: Hsla,
    pub pin: Hsla,
    /// 叠在缩略图/封面上的遮罩与悬停遮罩：固定在媒体上，不随主题明暗翻转。
    pub media_scrim: Hsla,
    pub media_scrim_hover: Hsla,
    // 附件
    pub tag_bg: Hsla,
    pub tag_ink: Hsla,
    pub tip: Hsla,
    pub scrim: Hsla,
    pub scroll_thumb: Hsla,
    pub scroll_thumb_hover: Hsla,
    pub dark: bool,
}

impl Palette {
    /// 当前主题的调色板。表格/列表每帧取色，构建一次后命中缓存。
    pub fn current(cx: &App) -> Self {
        thread_local! {
            static CACHE: std::cell::Cell<Option<(PaletteKey, Palette)>> =
                const { std::cell::Cell::new(None) };
        }
        let theme = cx.theme();
        let key = (
            theme.mode,
            theme.colors.primary,
            theme.colors.foreground,
            theme.colors.background,
        );
        if let Some((cached, palette)) = CACHE.get() {
            if cached == key {
                return palette;
            }
        }
        let palette = Self::build(cx);
        CACHE.set(Some((key, palette)));
        palette
    }

    fn build(cx: &App) -> Self {
        let theme = cx.theme();
        let dark = theme.mode == ThemeMode::Dark;
        let color = |key: &str| tokens::token(key, dark);
        Self {
            ink: theme.colors.foreground,
            ink_2: theme.colors.muted_foreground,
            ink_3: color("ink-3"),
            // 直读令牌：全局主题里的 `background` 被置为透明（窗口底由各视图
            // 自绘，DWM 圆角才能透出），直接继承会让快捷窗一类的自绘窗口没有底。
            background: color("window-bg"),
            layer: color("layer"),
            layer_strong: color("layer-strong"),
            flyout: color("flyout"),
            stroke: theme.colors.border,
            stroke_strong: color("stroke-strong"),
            hover: theme.colors.list_hover,
            hover_strong: color("hover-strong"),
            press: color("press"),
            control_face: color("control"),
            control_hover: color("control-hover"),
            control_checked: color("control-checked"),
            control_edge: remainder(color("stroke"), color("stroke-strong")),
            accent: theme.colors.primary,
            on_accent: theme.colors.primary_foreground,
            fav: color("fav"),
            danger: color("danger"),
            on_danger: color("on-danger"),
            warn: color("warn"),
            ok: color("ok"),
            pin: color("pin"),
            media_scrim: color("media-scrim"),
            media_scrim_hover: color("media-scrim-hover"),
            tag_bg: color("tag-bg"),
            tag_ink: color("tag-ink"),
            tip: color("tip"),
            scrim: color("scrim"),
            scroll_thumb: color("scroll-thumb"),
            scroll_thumb_hover: color("scroll-thumb-hover"),
            dark,
        }
    }

    /// 强调色 15% 底（选中卡片/激活标签）。
    pub fn accent_soft(&self) -> Hsla {
        motion::mix(self.layer, self.accent, 0.15)
    }

    /// 搜索命中块的底：强调色自身的不透明度。亮色模式压得更淡，否则浅底上
    /// 命中文字会糊；暗色模式反之。数值与卡片列表沿用同一口径
    /// （`theme.rs::accent_highlight`），两处高亮不会各自漂移。
    pub fn highlight(&self) -> Hsla {
        self.accent
            .opacity(if self.dark { 0.40 } else { 77. / 255. })
    }

    /// 命中文字：底色恒为强调色，所以按明暗取黑/白，不跟 `on_accent` 翻转。
    pub fn on_highlight(&self) -> Hsla {
        if self.dark {
            Hsla::white()
        } else {
            Hsla::black()
        }
    }

    /// 半透明控件面叠在卡片上的不透明等效色，避免透明窗口上多层叠加发暗。
    pub fn on_card(&self, name: &str) -> Hsla {
        let color = self
            .background
            .blend(self.layer)
            .blend(tokens::token(name, self.dark));
        Hsla { a: 1., ..color }
    }
}

/// 调色板缓存键：只依赖这些量的主题变化需要重建。
type PaletteKey = (ThemeMode, Hsla, Hsla, Hsla);

/// `under` 上叠 `over` 得到 `over` 时，`over` 自身的等效值（同色不同强度，
/// 描边由白/黑两种强度组成）。
fn remainder(under: Hsla, over: Hsla) -> Hsla {
    Hsla {
        a: 1. - (1. - over.a) / (1. - under.a),
        ..over
    }
}

/// 把令牌表灌进 gpui-kit 全局主题：输入框、滚动条、提示层、菜单随色板统一。
///
/// `accent` 为 `Some` 时优先（设置里选了系统强调色），否则读令牌。
pub fn apply(mode: ThemeMode, accent: Option<Rgba>, cx: &mut App) {
    Theme::change(mode, None, cx);
    let dark = mode == ThemeMode::Dark;
    let color = |key: &str| tokens::token(key, dark);
    let accent = accent.map(Hsla::from).unwrap_or_else(|| color("accent"));
    let sans = super::typography::installed_family("sans", cx);
    let base = super::typography::scaled_base();

    Theme::update(cx, |theme| {
        let mut t = theme.semantic_tokens();
        t.colors.background = color("window-bg");
        t.colors.foreground = color("ink");
        // 菜单/对话框把阴影画在不透明面下；半透明面会让阴影透上来发灰。
        t.colors.surface = Hsla {
            a: 1.,
            ..color("flyout")
        };
        t.colors.surface_foreground = color("ink");
        t.colors.primary = accent;
        t.colors.primary_foreground = color("on-accent");
        t.colors.secondary = color("control");
        t.colors.secondary_foreground = color("ink");
        t.colors.muted = color("layer");
        t.colors.muted_foreground = color("ink-2");
        t.colors.accent = color("hover");
        t.colors.accent_foreground = color("ink");
        t.colors.destructive = color("danger");
        t.colors.destructive_foreground = color("on-danger");
        t.colors.border = color("stroke");
        t.colors.input = color("control");
        t.colors.ring = accent;
        t.typography.md.size = base;
        t.typography.sans = sans;
        theme.apply_semantic_tokens(&t);

        // 旧字段：gpui-kit 的组件仍从这里取色。
        theme.colors.group_box = color("layer");
        theme.colors.list_hover = color("hover");
        theme.colors.table = color("layer").opacity(0.);
        theme.colors.table_head = color("layer").opacity(0.);
        theme.colors.table_head_foreground = color("ink-2");
        theme.colors.table_hover = color("hover");
        theme.colors.table_row_border = color("stroke").opacity(0.);
        theme.colors.table_active = accent.opacity(0.15);
        theme.colors.table_active_border = accent.opacity(0.);
        theme.list.active_highlight = true;
        theme.colors.tab_foreground = color("ink-2");
        theme.colors.tab_active_foreground = color("ink");
        theme.colors.tab_active = color("control-checked");
        theme.colors.tab_bar = color("control");
        theme.colors.tab_bar_segmented = color("window-bg").blend(color("control"));
        theme.colors.switch = color("ink-3");
        theme.colors.switch_thumb = color("window-bg");
        theme.colors.title_bar = color("window-bg").opacity(0.);
        theme.colors.title_bar_border = color("stroke").opacity(0.);
        // 对话框自带 scrim，组件 backdrop 只负责吃掉外部点击。
        theme.colors.overlay = color("scrim").opacity(0.);
        // 菜单、对话框 8px 圆角。
        theme.radius_lg = px(8.);
        // 透明窗口：底色由各视图自己的 window-bg 层负责（DWM 圆角/透明不变）。
        theme.colors.background = Hsla::transparent_black();
    });

    super::super::font::apply_to_global_theme(cx);
}

/// 在 `window` 存在时调用（启动路径），确保首帧前主题已就位。
pub fn apply_for_window(mode: ThemeMode, accent: Option<Rgba>, window: &mut Window, cx: &mut App) {
    Theme::change(mode, Some(window), cx);
    apply(mode, accent, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 相对亮度。
    fn luminance(color: Hsla) -> f32 {
        let rgba = color.to_rgb();
        let channel = |c: f32| {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.r) + 0.7152 * channel(rgba.g) + 0.0722 * channel(rgba.b)
    }

    fn contrast(fg: Hsla, bg: Hsla) -> f32 {
        let (fg, bg) = (luminance(fg), luminance(bg));
        (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05)
    }

    /// 把半透明面（卡片、浮层）叠到窗口底上，得到真正的底色。
    fn flattened(dark: bool, layer: Option<&str>) -> Hsla {
        let mut color = tokens::token("window-bg", dark);
        if let Some(name) = layer {
            color = color.blend(tokens::token(name, dark));
        }
        Hsla { a: 1., ..color }
    }

    fn assert_contrast(what: &str, fg: Hsla, bg: Hsla, min: f32) {
        let ratio = contrast(fg, bg);
        assert!(ratio >= min, "{what}: {ratio:.2}:1 < {min}:1");
    }

    /// FR-A11Y-03：正文 ≥ 4.5:1、辅助与图标 ≥ 3:1。底色覆盖窗口、卡片
    /// （`layer`）与浮层（`flyout`）三层，因为同一文字会落在三者上。
    #[test]
    fn text_and_icon_contrast_meets_wcag() {
        for dark in [false, true] {
            let mode = if dark { "dark" } else { "light" };
            let surfaces = [
                ("window", flattened(dark, None)),
                ("card", flattened(dark, Some("layer"))),
                ("flyout", flattened(dark, Some("flyout"))),
            ];
            let ink = tokens::token("ink", dark);
            let ink_2 = tokens::token("ink-2", dark);
            let ink_3 = tokens::token("ink-3", dark);
            let accent = tokens::token("accent", dark);
            let on_accent = tokens::token("on-accent", dark);
            let danger = tokens::token("danger", dark);
            let on_danger = tokens::token("on-danger", dark);
            let fav = tokens::token("fav", dark);

            for (surface, bg) in surfaces {
                assert_contrast(&format!("{mode} ink on {surface}"), ink, bg, 4.5);
                assert_contrast(&format!("{mode} ink-2 on {surface}"), ink_2, bg, 4.5);
                assert_contrast(&format!("{mode} ink-3 on {surface}"), ink_3, bg, 3.0);
                assert_contrast(&format!("{mode} accent on {surface}"), accent, bg, 4.5);
            }
            assert_contrast(
                &format!("{mode} on-accent on accent"),
                on_accent,
                accent,
                4.5,
            );
            assert_contrast(
                &format!("{mode} on-danger on danger"),
                on_danger,
                danger,
                4.5,
            );
            assert_contrast(
                &format!("{mode} danger on flyout"),
                danger,
                flattened(dark, Some("flyout")),
                4.5,
            );
            assert_contrast(
                &format!("{mode} fav icon on window"),
                fav,
                flattened(dark, None),
                3.0,
            );
        }
    }
}
