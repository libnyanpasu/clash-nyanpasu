use language_tags::LanguageTag;
use sys_locale::get_locale;

/// Get the system locale as a `LanguageTag`.
pub fn get_system_locale() -> LanguageTag {
    LanguageTag::parse(&get_locale().unwrap_or_else(|| "en-US".to_string()))
        .expect("system locale should a valid BEP47 language tag")
        .canonicalize()
        .expect("should a valid language tag")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_locale() {
        let locale = get_system_locale();
        eprintln!("System locale: {}", locale);
    }
}
