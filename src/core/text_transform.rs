//! 粘贴前的文本清理变换（「清理后粘贴」）。
//!
//! 变换只作用于**本次粘贴写入剪贴板的内容**，不修改历史条目本身：条目原文
//! 是用户复制的事实，改写它会让下次复制同一内容时无法去重，也会让历史记录
//! 与来源不一致。需要持久修改请用编辑面板。
//!
//! 所有函数都是纯逻辑（无 IO、无平台分支），便于单测。

/// 一次粘贴可用的清理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    /// 去掉所有换行，把多行拼成一行。
    RemoveLineBreaks,
    /// 每行去首尾空白，并丢弃空行。
    TrimLines,
    /// 只去掉整段文本的首尾空白。
    Trim,
    /// 转大写。
    Uppercase,
    /// 转小写。
    Lowercase,
}

impl TextTransform {
    /// 全部变换，顺序即菜单展示顺序。
    pub const ALL: [Self; 5] = [
        Self::RemoveLineBreaks,
        Self::TrimLines,
        Self::Trim,
        Self::Uppercase,
        Self::Lowercase,
    ];

    /// 稳定的持久化 id（写进设置，随版本变化也必须保持不变）。
    pub fn id(self) -> &'static str {
        match self {
            Self::RemoveLineBreaks => "remove_line_breaks",
            Self::TrimLines => "trim_lines",
            Self::Trim => "trim",
            Self::Uppercase => "uppercase",
            Self::Lowercase => "lowercase",
        }
    }

    /// 从持久化 id 还原；未知值返回 `None`（旧配置/被手改的配置文件）。
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "remove_line_breaks" => Some(Self::RemoveLineBreaks),
            "trim_lines" => Some(Self::TrimLines),
            "trim" => Some(Self::Trim),
            "uppercase" => Some(Self::Uppercase),
            "lowercase" => Some(Self::Lowercase),
            _ => None,
        }
    }

    /// 应用到一段文本。结果为空（或只剩空白）时返回 `None`，调用方据此提示
    /// 用户而不是往剪贴板写空字符串。
    pub fn apply(self, text: &str) -> Option<String> {
        let normalized = normalize_newlines(text);
        let result = match self {
            Self::RemoveLineBreaks => normalized.replace('\n', ""),
            Self::TrimLines => normalized
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Trim => normalized.trim().to_string(),
            Self::Uppercase => normalized.to_uppercase(),
            Self::Lowercase => normalized.to_lowercase(),
        };
        (!result.trim().is_empty()).then_some(result)
    }
}

/// CRLF / CR 统一成 LF，让「按行」的变换不受来源平台影响。
fn normalize_newlines(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_ids_are_rejected() {
        for transform in TextTransform::ALL {
            assert_eq!(TextTransform::from_id(transform.id()), Some(transform));
        }
        assert_eq!(TextTransform::from_id(""), None);
        assert_eq!(TextTransform::from_id("Trim"), None);
    }

    #[test]
    fn line_breaks_are_removed_after_newline_normalization() {
        assert_eq!(
            TextTransform::RemoveLineBreaks
                .apply("a\r\nb\rc\n")
                .unwrap(),
            "abc"
        );
    }

    #[test]
    fn trim_lines_drops_indentation_and_blank_lines() {
        assert_eq!(
            TextTransform::TrimLines
                .apply("  first  \r\n\r\n\tsecond\t\n\n")
                .unwrap(),
            "first\nsecond"
        );
    }

    #[test]
    fn trim_only_touches_the_ends() {
        assert_eq!(TextTransform::Trim.apply("  a  b  \n").unwrap(), "a  b");
    }

    #[test]
    fn case_transforms_preserve_inner_layout() {
        assert_eq!(TextTransform::Uppercase.apply("a\nb").unwrap(), "A\nB");
        assert_eq!(TextTransform::Lowercase.apply("A\nB").unwrap(), "a\nb");
    }

    #[test]
    fn empty_or_blank_results_are_reported_as_none() {
        for transform in TextTransform::ALL {
            assert_eq!(transform.apply(""), None, "{}", transform.id());
            assert_eq!(transform.apply("   \n\t"), None, "{}", transform.id());
        }
    }

    #[test]
    fn whitespace_only_lines_do_not_survive_trim_lines() {
        assert_eq!(
            TextTransform::TrimLines.apply("a\n \t \nb").unwrap(),
            "a\nb"
        );
    }
}
