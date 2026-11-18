//! 动效偏好的唯一入口：把设置里的两个开关折算成运行期状态。
//!
//! - 「界面动画」映射到 `App::set_reduce_motion`。仓库里所有过渡
//!   （`design::motion::transition`、`ui::transition::keyed_transition`、
//!   `widgets::PageEntrance` 与 gpui-kit 组件自带动效）都读这个标志，
//!   所以一处写入即可统一切换，不需要在各页面重复判设置。
//! - 「窗口过渡动画」写入 `platform::window_motion`，窗口显示/隐藏时读取。
//!
//! 两个开关都是 `Option<bool>`：`None` 跟随系统的「动画效果」。这里**不**用
//! `gpui_base::apply_system_reduce_motion`——应用一旦 `set_reduce_motion`
//! 过，gpui-base 就不再接受系统读数；每次自行读系统设置再写入，才能保证
//! 「关掉再改回跟随系统」仍然生效。

use gpui::App;

use crate::core::settings::AppSettings;

/// 界面动效是否开启（跟随系统时取系统「动画效果」）。
pub fn effective_ui_motion(settings: &AppSettings) -> bool {
    settings
        .ui_motion
        .unwrap_or_else(crate::platform::motion::system_animations_enabled)
}

/// 主窗口是否做淡入淡出（跟随系统时取系统「动画效果」）。
pub fn effective_window_motion(settings: &AppSettings) -> bool {
    settings
        .window_motion
        .unwrap_or_else(crate::platform::motion::system_animations_enabled)
}

/// 把当前设置应用到运行期。启动时调用一次，设置变更时再调用。
pub fn apply(settings: &AppSettings, cx: &mut App) {
    let ui = effective_ui_motion(settings);
    let window = effective_window_motion(settings);
    cx.set_reduce_motion(!ui);
    // 绘制期发生的动效（图标换名入场）读这个标志，与 reduce_motion 同步。
    crate::ui::icon_motion::set_enabled(ui);
    crate::platform::window_motion::set_enabled(window);
    log::info!(
        "motion: ui={ui} window={window} ui_motion={:?} window_motion={:?}",
        settings.ui_motion,
        settings.window_motion
    );
}

#[cfg(test)]
mod tests {
    use super::{effective_ui_motion, effective_window_motion};
    use crate::core::settings::AppSettings;

    #[test]
    fn explicit_choices_ignore_the_system_reading() {
        let off = AppSettings {
            ui_motion: Some(false),
            window_motion: Some(false),
            ..AppSettings::default()
        };
        assert!(!effective_ui_motion(&off));
        assert!(!effective_window_motion(&off));

        let on = AppSettings {
            ui_motion: Some(true),
            window_motion: Some(true),
            ..AppSettings::default()
        };
        assert!(effective_ui_motion(&on));
        assert!(effective_window_motion(&on));
    }

    #[test]
    fn following_the_system_reads_one_value_for_both_channels() {
        let settings = AppSettings::default();
        let system = crate::platform::motion::system_animations_enabled();
        assert_eq!(effective_ui_motion(&settings), system);
        assert_eq!(effective_window_motion(&settings), system);
    }
}
