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
//!
//! 第 1/2/4 条由本文件底部的门禁测试扫描源码执行：违反时 `cargo test` 直接
//! 失败，不再是"约定靠自觉"。

pub mod motion;
pub mod palette;
pub mod size;
pub mod squircle;
pub mod tokens;
pub mod typography;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// 页面层源码：`src/ui` 下排除 `design/`（令牌层自身）与
    /// `rich_preview.rs`（渲染用户 HTML，颜色来自剪贴板内容而非主题）。
    fn page_sources() -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).expect("read src/ui") {
                let path = entry.expect("src/ui entry").path();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if path.is_dir() {
                    if name != "design" {
                        walk(&path, out);
                    }
                } else if name.ends_with(".rs") && name != "rich_preview.rs" {
                    out.push(path);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        let mut sources = Vec::new();
        walk(&root, &mut sources);
        sources
    }

    /// 生产代码行。`#[cfg(test)]` 之后视为测试代码（本仓测试模块一律置于
    /// 文件末尾），测试里的假数据不受主题约束。
    fn production_lines(path: &Path) -> Vec<(usize, String)> {
        let text = fs::read_to_string(path).expect("read page source");
        text.lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.to_owned()))
            .take_while(|(_, line)| !line.trim_start().starts_with("#[cfg(test)]"))
            .collect()
    }

    #[test]
    fn page_layer_has_no_color_literals() {
        let mut violations = Vec::new();
        for path in page_sources() {
            for (line_no, line) in production_lines(&path) {
                if line.contains("rgb(0x") || line.contains("rgba(0x") {
                    violations.push(format!("{}:{line_no}: {}", path.display(), line.trim()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "颜色字面量必须写进 assets/theme.json：\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn rounded_values_come_from_the_scale() {
        const SCALE: [f32; 4] = [4., 6., 8., 10.];
        const NEEDLE: &str = "rounded(px(";
        let mut violations = Vec::new();
        for path in page_sources() {
            for (line_no, line) in production_lines(&path) {
                let mut search_from = 0;
                while let Some(offset) = line[search_from..].find(NEEDLE) {
                    let start = search_from + offset + NEEDLE.len();
                    let Some(end) = line[start..].find(')') else {
                        break;
                    };
                    let argument = &line[start..start + end];
                    // 计算值（`TRACK_HEIGHT / 2.0`、`panel_radius`）由调用点负责，
                    // 这里只拦写死的数值。
                    if let Ok(value) = argument.trim_end_matches('.').parse::<f32>() {
                        if !SCALE.contains(&value) {
                            violations.push(format!(
                                "{}:{line_no}: rounded(px({argument})",
                                path.display()
                            ));
                        }
                    }
                    search_from = start + end + 1;
                }
            }
        }
        assert!(
            violations.is_empty(),
            "圆角只允许 4/6/8/10（其余走 Radius::*）：\n{}",
            violations.join("\n")
        );
    }

    /// 缓动曲线只在 `design/motion.rs` 定义。`components/reorder.rs` 保留一个
    /// 转发壳（`ease_in_out` → `motion::ease_in_out_quad`），因此放行。
    #[test]
    fn page_layer_declares_no_easing() {
        let mut violations = Vec::new();
        for path in page_sources() {
            if path.ends_with("components/reorder.rs") {
                continue;
            }
            for (line_no, line) in production_lines(&path) {
                if line.contains(" fn ease") || line.contains("cubic_bezier(") {
                    violations.push(format!("{}:{line_no}: {}", path.display(), line.trim()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "缓动曲线只允许定义在 design/motion.rs：\n{}",
            violations.join("\n")
        );
    }
}
