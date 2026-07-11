use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum I18nLanguage {
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
}

pub fn default_i18n_language() -> I18nLanguage {
    let system_locale = nyanpasu_helper::locale::get_system_locale();
    if is_english(&system_locale) {
        I18nLanguage::English
    } else if is_korean(&system_locale) {
        I18nLanguage::Korean
    } else if is_russian(&system_locale) {
        I18nLanguage::Russian
    } else if is_simplified_chinese(&system_locale) {
        I18nLanguage::SimplifiedChinese
    } else if is_traditional_chinese(&system_locale) {
        I18nLanguage::TraditionalChinese
    } else {
        I18nLanguage::English
    }
}

fn is_english(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("en")
}

fn is_korean(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ko")
}

fn is_russian(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ru")
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

fn is_traditional_chinese(lang: &LanguageTag) -> bool {
    if !lang.primary_language().eq_ignore_ascii_case("zh") {
        return false;
    }
    // Prefer the explicit script subtag when present.
    match lang.script() {
        Some(script) if script.eq_ignore_ascii_case("Hant") => return true,
        Some(script) if script.eq_ignore_ascii_case("Hans") => return false,
        _ => {}
    }
    // Fall back to the region: only TW/HK/MO are Traditional Chinese.
    // Bare `zh` and Simplified regions (CN/SG/MY) default to Simplified.
    match lang.region() {
        Some(region) => matches!(region.to_ascii_uppercase().as_str(), "TW" | "HK" | "MO"),
        None => false,
    }
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
    fn detects_traditional_chinese() {
        assert!(is_traditional_chinese(&tag("zh-TW")));
        assert!(is_traditional_chinese(&tag("zh-HK")));
        assert!(is_traditional_chinese(&tag("zh-MO")));
        assert!(is_traditional_chinese(&tag("zh-Hant")));
        assert!(is_traditional_chinese(&tag("zh-Hant-TW")));
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
    fn detects_english_korean_and_russian() {
        assert!(is_english(&tag("en")));
        assert!(is_english(&tag("en-US")));
        assert!(is_english(&tag("en-GB")));
        assert!(is_korean(&tag("ko")));
        assert!(is_korean(&tag("ko-KR")));
        assert!(is_russian(&tag("ru")));
        assert!(is_russian(&tag("ru-RU")));
        assert!(!is_english(&tag("zh-CN")));
        assert!(!is_russian(&tag("en-US")));
    }
}
