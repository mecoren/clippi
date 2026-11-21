//! 指针动作：左键单击 / 左键双击 / 中键分别做什么。
//!
//! 旧版本只有 `paste_click_mode`（`double_click` / `single_click`），两个取值
//! 都表示「粘贴」，区别只是用哪种手势。这里把它扩展成「按键 → 动作」的映射，
//! 并在新配置缺失时从旧字段推导，保证升级后行为不变。
//!
//! 纯逻辑，便于单测。

/// 一次指针动作要做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    /// 不做任何动作（只选中条目）。
    None,
    /// 只复制到剪贴板，不模拟粘贴。
    Copy,
    /// 写剪贴板并模拟粘贴。
    Paste,
}

impl ClickAction {
    /// 全部取值，顺序即设置页展示顺序。
    pub const ALL: [Self; 3] = [Self::None, Self::Copy, Self::Paste];

    /// 稳定的持久化 id。
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::Paste => "paste",
        }
    }

    /// 从持久化 id 还原；未知值返回 `None`（交给调用方回退）。
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "none" => Some(Self::None),
            "copy" => Some(Self::Copy),
            "paste" => Some(Self::Paste),
            _ => None,
        }
    }
}

/// 三种指针动作的解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickActions {
    pub single: ClickAction,
    pub double: ClickAction,
    pub middle: ClickAction,
}

/// 解析配置值。
///
/// - 三个新字段**都不为空**时以它们为准（未知值按 `None` 处理）。
/// - 全为空说明是升级前的配置，按旧的 `paste_click_mode` 推导：
///   `single_click` → 单击粘贴、双击无动作；其余 → 单击无动作、双击粘贴。
/// - 中键在任何历史配置里都没有行为，默认 `None`，不会因升级突然改变。
pub fn resolve(
    single: &str,
    double: &str,
    middle: &str,
    legacy_paste_click_mode: &str,
) -> ClickActions {
    let configured = !single.is_empty() || !double.is_empty() || !middle.is_empty();
    if configured {
        return ClickActions {
            single: ClickAction::from_id(single).unwrap_or(ClickAction::None),
            double: ClickAction::from_id(double).unwrap_or(ClickAction::None),
            middle: ClickAction::from_id(middle).unwrap_or(ClickAction::None),
        };
    }
    if legacy_paste_click_mode == "single_click" {
        ClickActions {
            single: ClickAction::Paste,
            double: ClickAction::None,
            middle: ClickAction::None,
        }
    } else {
        ClickActions {
            single: ClickAction::None,
            double: ClickAction::Paste,
            middle: ClickAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for action in ClickAction::ALL {
            assert_eq!(ClickAction::from_id(action.id()), Some(action));
        }
        assert_eq!(ClickAction::from_id(""), None);
        assert_eq!(ClickAction::from_id("Paste"), None);
    }

    #[test]
    fn legacy_double_click_maps_to_double_paste_only() {
        let actions = resolve("", "", "", "double_click");
        assert_eq!(actions.single, ClickAction::None);
        assert_eq!(actions.double, ClickAction::Paste);
        assert_eq!(actions.middle, ClickAction::None);
    }

    #[test]
    fn legacy_single_click_maps_to_single_paste_only() {
        let actions = resolve("", "", "", "single_click");
        assert_eq!(actions.single, ClickAction::Paste);
        assert_eq!(actions.double, ClickAction::None);
        assert_eq!(actions.middle, ClickAction::None);
    }

    #[test]
    fn unknown_legacy_mode_falls_back_to_double_click() {
        let actions = resolve("", "", "", "triple_click");
        assert_eq!(actions.double, ClickAction::Paste);
    }

    #[test]
    fn configured_values_win_over_the_legacy_field() {
        let actions = resolve("copy", "paste", "paste", "single_click");
        assert_eq!(actions.single, ClickAction::Copy);
        assert_eq!(actions.double, ClickAction::Paste);
        assert_eq!(actions.middle, ClickAction::Paste);
    }

    #[test]
    fn partially_configured_values_leave_other_gestures_inert() {
        // 只要配置过一项，就不再从旧字段推导，避免「配了中键却顺带改了左键」。
        let actions = resolve("", "", "copy", "single_click");
        assert_eq!(actions.single, ClickAction::None);
        assert_eq!(actions.double, ClickAction::None);
        assert_eq!(actions.middle, ClickAction::Copy);
    }

    #[test]
    fn unknown_configured_value_is_inert() {
        let actions = resolve("nope", "paste", "nope", "double_click");
        assert_eq!(actions.single, ClickAction::None);
        assert_eq!(actions.double, ClickAction::Paste);
        assert_eq!(actions.middle, ClickAction::None);
    }
}
