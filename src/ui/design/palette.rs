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
    pub pin: Hsla,
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
            background: theme.colors.background,
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
            pin: color("pin"),
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
