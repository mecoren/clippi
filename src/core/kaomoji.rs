//! Offline kaomoji data; category IDs match scripts/update-kaomoji-data.py.
use super::emoji::EmojiEntry;
use super::i18n_keys::I18nKey;
use std::sync::OnceLock;

pub const CATEGORIES: &[(I18nKey, &str)] = &[
    (I18nKey::KaomojiCategoryCute, "♡"),
    (I18nKey::KaomojiCategorySad, "☂"),
    (I18nKey::KaomojiCategoryHeadpats, "♡"),
    (I18nKey::KaomojiCategoryPeeking, "◉"),
    (I18nKey::KaomojiCategoryClassics, "☆"),
    (I18nKey::KaomojiCategoryTableFlips, "ϟ"),
    (I18nKey::KaomojiCategoryFood, "☕"),
    (I18nKey::KaomojiCategoryAnimals, "ω"),
    (I18nKey::KaomojiCategoryHappy, "☺"),
    (I18nKey::KaomojiCategoryShy, "⁄"),
    (I18nKey::KaomojiCategorySurprised, "!"),
    (I18nKey::KaomojiCategoryMix, "✦"),
    (I18nKey::KaomojiCategorySoft, "♡"),
    (I18nKey::KaomojiCategorySmiling, "☺"),
    (I18nKey::KaomojiCategorySparkles, "✧"),
    (I18nKey::KaomojiCategoryFandom, "☆"),
    (I18nKey::KaomojiCategoryLove, "♥"),
    (I18nKey::KaomojiCategoryPlayful, "♪"),
    (I18nKey::KaomojiCategoryFalling, "↓"),
    (I18nKey::KaomojiCategoryAsciiArt, "▧"),
    (I18nKey::KaomojiCategorySleepy, "☾"),
    (I18nKey::KaomojiCategoryAngry, "ϟ"),
    (I18nKey::KaomojiCategoryCheering, "⚑"),
    (I18nKey::KaomojiCategoryGreetings, "✌"),
];

pub fn entries() -> &'static [EmojiEntry] {
    static DATA: OnceLock<Vec<EmojiEntry>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/emoji/kaomoji.json"))
            .expect("embedded kaomoji data must be valid")
    })
}

pub fn in_category(entry: &EmojiEntry, category: usize) -> bool {
    entry.groups.contains(&category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaomoji_are_unique_single_line_and_cover_all_categories() {
        let data = entries();
        assert!(data.len() > 1500);
        let unique: std::collections::HashSet<_> = data.iter().map(|e| &e.text).collect();
        assert_eq!(unique.len(), data.len());
        assert!(data.iter().all(|e| !e.text.trim().is_empty()
            && !e.text.chars().any(char::is_control)
            && !e.name.is_empty()
            && !e.name_en.is_empty()
            && !e.groups.is_empty()
            && e.groups.iter().all(|&g| g < CATEGORIES.len())));
        for category in 0..CATEGORIES.len() {
            assert!(data.iter().any(|e| in_category(e, category)));
        }
    }

    #[test]
    fn bilingual_search_and_overlapping_categories_survive_deduplication() {
        let data = entries();
        for query in [
            "开心",
            "happy",
            "摸头",
            "headpat",
            "睡觉",
            "sleepy",
            "掀桌",
            "table flip",
            "你好",
            "hello",
        ] {
            assert!(data.iter().any(|e| e.matches(query)), "missing {query}");
        }
        let shared = data.iter().find(|e| e.groups.len() > 1).unwrap();
        for &category in &shared.groups {
            assert!(in_category(shared, category));
        }
        assert!(data.iter().any(|e| e.text == "(╯°□°)╯︵ ┻━┻"));
        assert!(data.iter().any(|e| e.text == "|ω・)"));
    }
}
