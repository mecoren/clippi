//! Clippi 设计系统（对齐 Argus `frontend/theme.rs` + `widgets.rs` 的分层）。
//!
//! # 分层铁律
//!
//! 1. 颜色字面量只允许出现在 `assets/theme.json`；`src/` 下不得新增
//!    `rgb(0x…)` / `rgba(0x…)` 字面量（`#[cfg(test)]` 与 `design/tokens.rs`
//!    的解析器除外）。
//! 2. 缓动与混色只允许定义在 `design/motion.rs`；页面不得自写 `|t| …`。
//! 3. 页面层取色走 [`palette::Palette::current`]，不直接调 [`tokens::token`]。
//! 4. 圆角/间距/控件高度走 [`size::Radius`] / [`size::Space`] / [`size::Ctrl`]。
//! 5. 字号走 [`typography::font`]（叠加用户字号缩放），不写 `px(13.)`。
//! 6. 组件（按钮/输入/开关/分段/菜单/对话框）经 `ui/widgets.rs` 工厂构造，
//!    不直接拼装 gpui-kit 原生控件。

// 令牌/控件 API 在逐屏改写（M2+）期间才逐步被页面消费；过渡期允许未引用。
#[allow(dead_code)]
pub mod motion;
#[allow(dead_code)]
pub mod palette;
#[allow(dead_code)]
pub mod size;
#[allow(dead_code)]
pub mod tokens;
#[allow(dead_code)]
pub mod typography;
