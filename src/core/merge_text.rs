//! 「合并条目」与「批量粘贴」共用的分隔符策略。
//!
//! 两条路径都用同一个设置：合并生成的新条目用它连接各段文本，批量粘贴用它
//! 作为两次粘贴之间的分隔。这样用户只需要理解一种行为。
//!
//! 纯逻辑（无 IO），便于单测。

/// 默认分隔符：与引入该设置之前的行为一致（换行）。
pub const DEFAULT_MODE: &str = "newline";

/// 全部可选模式，顺序即设置页展示顺序。
pub const MODES: [&str; 4] = ["newline", "space", "comma", "none"];

/// 未知值一律按默认模式处理，避免旧配置或手改配置导致空连接。
pub fn normalize_mode(mode: &str) -> &'static str {
    match mode {
        "space" => "space",
        "comma" => "comma",
        "none" => "none",
        _ => DEFAULT_MODE,
    }
}

/// 分隔符的纯文本形式。
pub fn separator(mode: &str) -> &'static str {
    match normalize_mode(mode) {
        "space" => " ",
        "comma" => ", ",
        "none" => "",
        _ => "\n",
    }
}

/// 分隔符的 HTML 形式（富文本合并时插在 `<body>` 片段之间）。
pub fn html_separator(mode: &str) -> &'static str {
    match normalize_mode(mode) {
        "space" => " ",
        "comma" => ", ",
        "none" => "",
        _ => "<br>",
    }
}

/// 按模式连接若干段文本。
pub fn join_texts<'a>(texts: impl IntoIterator<Item = &'a str>, mode: &str) -> String {
    let sep = separator(mode);
    let mut result = String::new();
    for (index, text) in texts.into_iter().enumerate() {
        if index > 0 {
            result.push_str(sep);
        }
        result.push_str(text);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_modes_fall_back_to_newline() {
        assert_eq!(normalize_mode(""), DEFAULT_MODE);
        assert_eq!(normalize_mode("semicolon"), DEFAULT_MODE);
        assert_eq!(separator("semicolon"), "\n");
        assert_eq!(html_separator("semicolon"), "<br>");
    }

    #[test]
    fn separators_match_their_mode() {
        assert_eq!(separator("newline"), "\n");
        assert_eq!(separator("space"), " ");
        assert_eq!(separator("comma"), ", ");
        assert_eq!(separator("none"), "");
        assert_eq!(html_separator("newline"), "<br>");
        assert_eq!(html_separator("none"), "");
    }

    #[test]
    fn join_places_separator_only_between_entries() {
        let texts = ["a", "b", "c"];
        assert_eq!(join_texts(texts, "newline"), "a\nb\nc");
        assert_eq!(join_texts(texts, "space"), "a b c");
        assert_eq!(join_texts(texts, "comma"), "a, b, c");
        assert_eq!(join_texts(texts, "none"), "abc");
    }

    #[test]
    fn join_handles_zero_and_one_entry() {
        let empty: [&str; 0] = [];
        assert_eq!(join_texts(empty, "comma"), "");
        assert_eq!(join_texts(["only"], "comma"), "only");
    }

    #[test]
    fn empty_entries_still_produce_a_separator() {
        // 空条目不该被静默跳过：用户选了两条，就该看到两条之间的分隔。
        assert_eq!(join_texts(["a", "", "b"], "comma"), "a, , b");
    }
}
