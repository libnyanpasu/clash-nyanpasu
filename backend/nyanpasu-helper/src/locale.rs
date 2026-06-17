use language_tags::LanguageTag;
use sys_locale::get_locale;

/// Get the system locale as a `LanguageTag`.
///
/// Falls back to `en-US` when the system reports no locale or a value that is
/// not a valid BCP47 language tag (e.g. the `C` / `POSIX` locale common in CI
/// and minimal container environments).
pub fn get_system_locale() -> LanguageTag {
    locale_or_fallback(get_locale().as_deref())
}

/// Parse and canonicalize a raw locale string, falling back to `en-US` when it
/// is absent or not a valid BCP47 language tag.
///
// FIXME(sys-locale): `sys_locale::get_locale` documents that it returns a
// BCP47 tag, but its Unix backend does not enforce this. It merely transforms
// the `LANG` / `LC_*` environment variables (stripping `.<codeset>` / `@<mod>`
// and turning `_` into `-`) without validating the grammar or filtering the
// POSIX `C` / `POSIX` locales. On minimal Linux/CI environments where the
// locale is `C` / `C.UTF-8`, it returns `Some("C")` — a single-letter primary
// subtag that is *not* a valid BCP47 language tag — instead of `None`. We must
// therefore parse defensively here rather than trusting the documented
// contract. Revisit if a future `sys-locale` release returns `None` for the
// POSIX locale.
fn locale_or_fallback(raw: Option<&str>) -> LanguageTag {
    raw.and_then(|raw| LanguageTag::parse(raw).ok()?.canonicalize().ok())
        .unwrap_or_else(|| {
            LanguageTag::parse("en-US").expect("`en-US` is a valid BCP47 language tag")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_locale() {
        // Must never panic regardless of the host locale.
        let locale = get_system_locale();
        eprintln!("System locale: {}", locale);
    }

    #[test]
    fn invalid_or_absent_locale_falls_back_to_en_us() {
        let fallback = LanguageTag::parse("en-US").unwrap();
        // The `C` locale (common in CI) has a single-letter primary subtag,
        // which is not a valid BCP47 language tag.
        assert_eq!(locale_or_fallback(Some("C")), fallback);
        assert_eq!(locale_or_fallback(Some("C.UTF-8")), fallback);
        assert_eq!(locale_or_fallback(Some("")), fallback);
        assert_eq!(locale_or_fallback(None), fallback);
    }

    #[test]
    fn valid_locale_is_preserved() {
        assert_eq!(
            locale_or_fallback(Some("zh-CN")),
            LanguageTag::parse("zh-CN").unwrap()
        );
    }
}
