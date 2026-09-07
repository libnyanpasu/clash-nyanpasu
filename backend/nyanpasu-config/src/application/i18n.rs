use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use specta::Type;

/// UI language of the application.
///
/// The serialized form is the canonical i18n key shared by every layer that
/// names a language: the `rust_i18n` bundles under `backend/tauri/locales`, the
/// paraglide runtime under `frontend/nyanpasu/src/paraglide`, and the dayjs
/// locale imports. All of those are lowercase, so this enum is too. Legacy
/// mixed-case spellings are still accepted on read through `serde(alias)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum I18nLanguage {
    #[serde(rename = "en", alias = "en-US")]
    English,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "zh-cn", alias = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-tw", alias = "zh-TW")]
    TraditionalChinese,
}

impl I18nLanguage {
    /// The canonical i18n key. Identical to the serialized representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Korean => "ko",
            Self::Russian => "ru",
            Self::SimplifiedChinese => "zh-cn",
            Self::TraditionalChinese => "zh-tw",
        }
    }
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

    const ALL: [I18nLanguage; 5] = [
        I18nLanguage::English,
        I18nLanguage::Korean,
        I18nLanguage::Russian,
        I18nLanguage::SimplifiedChinese,
        I18nLanguage::TraditionalChinese,
    ];

    #[test]
    fn as_str_matches_serialized_form() {
        for lang in ALL {
            let serialized = serde_json::to_string(&lang).unwrap();
            assert_eq!(serialized, format!("\"{}\"", lang.as_str()));
        }
    }

    #[test]
    fn canonical_keys_are_lowercase() {
        // The keys must stay byte-identical to `backend/tauri/locales/<key>.json`
        // and to the paraglide locale list, both of which are lowercase.
        for lang in ALL {
            assert_eq!(lang.as_str(), lang.as_str().to_ascii_lowercase());
        }
    }

    #[test]
    fn legacy_mixed_case_spellings_still_deserialize() {
        let parse = |s: &str| serde_json::from_str::<I18nLanguage>(&format!("\"{s}\"")).unwrap();
        assert_eq!(parse("en-US"), I18nLanguage::English);
        assert_eq!(parse("zh-CN"), I18nLanguage::SimplifiedChinese);
        assert_eq!(parse("zh-TW"), I18nLanguage::TraditionalChinese);
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
