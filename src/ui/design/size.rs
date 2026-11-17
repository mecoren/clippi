//! 圆角 / 间距 / 控件高度档位：4pt 网格上的有限枚举。
//!
//! 页面层只写 `radius(Radius::LG)` / `gap(Space::MD)`，不写像素字面量
//! （Argus 同款分层，见 `docs/ui-redesign/design.md` §5）。

use gpui::{px, Pixels};

/// 圆角档位（`layout` 令牌）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radius {
    /// 4：小控件、keycaps、缩略图
    Sm,
    /// 6：按钮、输入框、设置分组卡
    Md,
    /// 8：菜单、对话框、浮层面板
    Lg,
    /// 10：剪贴板卡片（产品身份，最大层级）
    Card,
    /// 999：pill（搜索框"新建"按钮、chips）
    Pill,
}

impl Radius {
    pub fn px(self) -> Pixels {
        match self {
            Radius::Sm => px(4.),
            Radius::Md => px(6.),
            Radius::Lg => px(8.),
            Radius::Card => px(10.),
            Radius::Pill => px(999.),
        }
    }

    /// 跟随用户字号设置的像素值。页面与卡片用它，与既有的 `sc()` 一致；
    /// 只有托盘、菜单这类不参与缩放的固定尺寸才用 [`Self::px`]。
    pub fn scaled(self) -> Pixels {
        super::super::font::sc(f32::from(self.px()))
    }
}

/// 间距档位（4pt 网格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// 2
    Xxs,
    /// 4
    Xs,
    /// 6
    Sm,
    /// 8
    Md,
    /// 12
    Lg,
    /// 16
    Xl,
    /// 24
    Xxl,
}

impl Space {
    pub fn px(self) -> Pixels {
        match self {
            Space::Xxs => px(2.),
            Space::Xs => px(4.),
            Space::Sm => px(6.),
            Space::Md => px(8.),
            Space::Lg => px(12.),
            Space::Xl => px(16.),
            Space::Xxl => px(24.),
        }
    }

    /// 跟随用户字号设置的像素值。
    pub fn scaled(self) -> Pixels {
        super::super::font::sc(f32::from(self.px()))
    }
}

/// 控件高度档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctrl {
    /// 22：过滤条 chip、工具栏小按钮
    Chip,
    /// 24：小按钮
    Small,
    /// 28：搜索框
    Input,
    /// 32：按钮、设置输入框
    Btn,
}

impl Ctrl {
    pub fn px(self) -> Pixels {
        match self {
            Ctrl::Chip => px(22.),
            Ctrl::Small => px(24.),
            Ctrl::Input => px(28.),
            Ctrl::Btn => px(32.),
        }
    }

    /// 跟随用户字号设置的像素值。
    pub fn scaled(self) -> Pixels {
        super::super::font::sc(f32::from(self.px()))
    }
}

/// 图标尺寸档位（`layout.iconSm/Md/Lg`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Sm,
    Md,
    Lg,
}

impl IconSize {
    pub fn px(self) -> Pixels {
        match self {
            IconSize::Sm => super::tokens::dimension("iconSm"),
            IconSize::Md => super::tokens::dimension("iconMd"),
            IconSize::Lg => super::tokens::dimension("iconLg"),
        }
    }

    /// 跟随用户字号设置的像素值。
    pub fn scaled(self) -> Pixels {
        super::super::font::sc(f32::from(self.px()))
    }
}
