use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum I18nLanguage {
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "ru")]
    Russian,
}

pub fn default_i18n_language() -> I18nLanguage {
    let system_locale = nyanpasu_helper::locale::get_system_locale();
    if is_simplified_chinese(&system_locale) {
        I18nLanguage::SimplifiedChinese
    } else if is_english(&system_locale) {
        I18nLanguage::English
    } else if is_russian(&system_locale) {
        I18nLanguage::Russian
    } else {
        I18nLanguage::English
    }
}

fn is_simplified_chinese(lang: &LanguageTag) -> bool {
    if !lang.primary_language().eq_ignore_ascii_case("zh") {
        return false;
    }
    // Prefer the explicit script subtag when present.
    match lang.script() {
        Some(script) if script.eq_ignore_ascii_case("Hans") => return true,
        Some(script) if script.eq_ignore_ascii_case("Hant") => return false,
        _ => {}
    }
    // Fall back to the region: only TW/HK/MO are Traditional Chinese.
    // Bare `zh` and Simplified regions (CN/SG/MY) default to Simplified.
    match lang.region() {
        Some(region) => !matches!(region.to_ascii_uppercase().as_str(), "TW" | "HK" | "MO"),
        None => true,
    }
}

fn is_english(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("en")
}

fn is_russian(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ru")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LanguageTag {
        LanguageTag::parse(s).unwrap()
    }

    #[test]
    fn detects_simplified_chinese() {
        assert!(is_simplified_chinese(&tag("zh-CN")));
        assert!(is_simplified_chinese(&tag("zh-Hans")));
        assert!(is_simplified_chinese(&tag("zh-Hans-CN")));
        assert!(is_simplified_chinese(&tag("zh-SG")));
        assert!(is_simplified_chinese(&tag("zh")));
    }

    #[test]
    fn rejects_traditional_chinese() {
        assert!(!is_simplified_chinese(&tag("zh-TW")));
        assert!(!is_simplified_chinese(&tag("zh-HK")));
        assert!(!is_simplified_chinese(&tag("zh-MO")));
        assert!(!is_simplified_chinese(&tag("zh-Hant")));
        assert!(!is_simplified_chinese(&tag("zh-Hant-TW")));
    }

    #[test]
    fn detects_english_and_russian() {
        assert!(is_english(&tag("en")));
        assert!(is_english(&tag("en-US")));
        assert!(is_english(&tag("en-GB")));
        assert!(is_russian(&tag("ru")));
        assert!(is_russian(&tag("ru-RU")));
        assert!(!is_english(&tag("zh-CN")));
        assert!(!is_russian(&tag("en-US")));
    }
}
