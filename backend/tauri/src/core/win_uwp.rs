#![cfg(target_os = "windows")]

use anyhow::{Result, bail};
use deelevate::{PrivilegeLevel, Token};
use nyanpasu_config::application::I18nLanguage;
use runas::Command as RunasCommand;
use std::{path::Path, process::Command as StdCommand};

fn tool_language(language: I18nLanguage) -> &'static str {
    match language {
        I18nLanguage::English => "en",
        I18nLanguage::SimplifiedChinese => "zh-cn",
        I18nLanguage::TraditionalChinese => "zh-tw",
        I18nLanguage::Japanese => "ja",
        I18nLanguage::Korean => "ko",
        I18nLanguage::Russian => "ru",
    }
}

/// The tool checks its own Windows App Runtime and offers to install it, so it is
/// launched without a runtime check here.
pub async fn invoke_uwptools(resource_dir: &Path, language: I18nLanguage) -> Result<()> {
    let tool_path = resource_dir.join("enableLoopback.exe");

    if !tool_path.exists() {
        bail!("enableLoopback exe not found");
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;

    let args = ["--lang", tool_language(language)];
    match level {
        PrivilegeLevel::NotPrivileged => RunasCommand::new(tool_path).args(&args).status()?,
        _ => StdCommand::new(tool_path).args(args).status()?,
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_application_language_to_a_supported_tool_language() {
        for (language, expected) in [
            (I18nLanguage::English, "en"),
            (I18nLanguage::SimplifiedChinese, "zh-cn"),
            (I18nLanguage::TraditionalChinese, "zh-tw"),
            (I18nLanguage::Japanese, "ja"),
            (I18nLanguage::Korean, "ko"),
            (I18nLanguage::Russian, "ru"),
        ] {
            assert_eq!(tool_language(language), expected);
        }
    }
}
