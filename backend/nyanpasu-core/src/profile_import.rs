//! Suggests what a profile downloaded from a URL is, from its response headers
//! and its content. The suggestion only preselects a choice the user confirms;
//! the import still validates the content against the kind they pick.

use nyanpasu_config::profile::{ScriptRuntime, TransformKind};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;

/// What a downloaded profile can be imported as.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    specta::Type,
    strum::VariantArray,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileImportKind {
    /// A Clash config file.
    Config,
    /// A merge: a YAML overlay applied to a config.
    Overlay,
    #[serde(rename = "javascript")]
    JavaScript,
    Lua,
}

impl ProfileImportKind {
    /// The transform the kind imports as; a config is no transform.
    pub fn transform(self) -> Option<TransformKind> {
        match self {
            Self::Config => None,
            Self::Overlay => Some(TransformKind::Overlay),
            Self::JavaScript => Some(TransformKind::Script {
                runtime: ScriptRuntime::JavaScript,
            }),
            Self::Lua => Some(TransformKind::Script {
                runtime: ScriptRuntime::Lua,
            }),
        }
    }
}

/// A signal that a downloaded profile is of some kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportSignal {
    /// `subscription-userinfo` carries the traffic quota of a subscription.
    SubscriptionQuota,
    /// `profile-title` names a subscription.
    ProfileTitle,
    /// `profile-update-interval` asks a subscription to be refreshed.
    UpdateInterval,
    /// The extension of the file name in `Content-Disposition` or the URL path.
    FileExtension,
    /// The media type in `Content-Type`.
    ContentType,
    /// A YAML mapping with `proxies` or `proxy-providers`.
    ProxiesKey,
    /// A YAML mapping with `prepend-`, `append-`, `override__` or `filter__` keys.
    OverlayDirectives,
    /// Lines that read as the script language.
    ScriptSyntax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ImportEvidence {
    pub kind: ProfileImportKind,
    pub signal: ImportSignal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ImportClassification {
    /// None when the signals do not settle on a kind.
    pub suggested: Option<ProfileImportKind>,
    /// Every signal found, whether or not it decided the suggestion.
    pub evidence: Vec<ImportEvidence>,
}

/// What a download reveals about a profile.
#[derive(Debug, Clone, Copy)]
pub struct ImportFacts<'a> {
    pub content: &'a str,
    pub url_path: &'a str,
    pub content_type: Option<&'a str>,
    /// The file name in `Content-Disposition`.
    pub file_name: Option<&'a str>,
    pub has_quota: bool,
    pub has_title: bool,
    pub has_update_interval: bool,
}

const OVERLAY_PREFIXES: [&str; 6] = [
    "prepend-",
    "prepend__",
    "append-",
    "append__",
    "override__",
    "filter__",
];

/// Suggests a kind by these rules, in order:
///
/// 1. A YAML mapping with proxies or overlay directives is a config when it
///    has proxies and either a subscription header or no directives, and an
///    overlay otherwise.
/// 2. Otherwise a script hint (extension or media type) decides, then the
///    script syntax when one language reads clearly more than the other.
/// 3. Otherwise any other YAML mapping is an overlay.
pub fn classify(facts: &ImportFacts<'_>) -> ImportClassification {
    let mut evidence = Vec::new();
    let mut add = |kind, signal| evidence.push(ImportEvidence { kind, signal });

    let has_subscription_header = facts.has_quota || facts.has_title || facts.has_update_interval;
    for (present, signal) in [
        (facts.has_quota, ImportSignal::SubscriptionQuota),
        (facts.has_title, ImportSignal::ProfileTitle),
        (facts.has_update_interval, ImportSignal::UpdateInterval),
    ] {
        if present {
            add(ProfileImportKind::Config, signal);
        }
    }

    let extension = facts
        .file_name
        .and_then(script_kind_of_path)
        .or_else(|| script_kind_of_path(facts.url_path));
    if let Some(kind) = extension {
        add(kind, ImportSignal::FileExtension);
    }
    let media = facts.content_type.and_then(script_kind_of_media_type);
    if let Some(kind) = media {
        add(kind, ImportSignal::ContentType);
    }

    let mapping = serde_yaml_ng::from_str::<Mapping>(facts.content).ok();
    let has_proxies = mapping.as_ref().is_some_and(|mapping| {
        mapping.contains_key("proxies") || mapping.contains_key("proxy-providers")
    });
    let has_directives = mapping.as_ref().is_some_and(has_overlay_directives);
    if has_proxies {
        add(ProfileImportKind::Config, ImportSignal::ProxiesKey);
    }
    if has_directives {
        add(ProfileImportKind::Overlay, ImportSignal::OverlayDirectives);
    }

    let syntax = script_kind_of_syntax(facts.content);
    if let Some(kind) = syntax {
        add(kind, ImportSignal::ScriptSyntax);
    }

    let suggested = if has_proxies || has_directives {
        Some(
            if has_proxies && (has_subscription_header || !has_directives) {
                ProfileImportKind::Config
            } else {
                ProfileImportKind::Overlay
            },
        )
    } else if let Some(kind) = extension.or(media).or(syntax) {
        Some(kind)
    } else if mapping.is_some() {
        Some(ProfileImportKind::Overlay)
    } else {
        None
    };

    ImportClassification {
        suggested,
        evidence,
    }
}

fn has_overlay_directives(mapping: &Mapping) -> bool {
    mapping.keys().filter_map(|key| key.as_str()).any(|key| {
        let key = key.to_ascii_lowercase();
        OVERLAY_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
    })
}

fn script_kind_of_path(path: &str) -> Option<ProfileImportKind> {
    let (_, extension) = path.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "js" | "mjs" | "cjs" => Some(ProfileImportKind::JavaScript),
        "lua" => Some(ProfileImportKind::Lua),
        _ => None,
    }
}

fn script_kind_of_media_type(media_type: &str) -> Option<ProfileImportKind> {
    let media_type = media_type.to_ascii_lowercase();
    if media_type.contains("javascript") || media_type.contains("ecmascript") {
        Some(ProfileImportKind::JavaScript)
    } else if media_type.contains("lua") {
        Some(ProfileImportKind::Lua)
    } else {
        None
    }
}

/// Counts the lines that only one of the two languages writes. Both write
/// `function main(`, so a function line counts by how it opens its body.
fn script_kind_of_syntax(content: &str) -> Option<ProfileImportKind> {
    let (mut lua, mut javascript) = (0usize, 0usize);
    for line in content.lines().map(str::trim) {
        if line.starts_with("--") || line.starts_with("local ") || line.contains("~=") {
            lua += 1;
        }
        if line == "end" || line.starts_with("end ") || line.starts_with("elseif ") {
            lua += 1;
        }
        if line.ends_with(" then") || line.ends_with(" do") {
            lua += 1;
        }
        if line.starts_with("//")
            || line.starts_with("const ")
            || line.starts_with("let ")
            || line.contains("=>")
            || line.contains("===")
            || line.contains("!==")
        {
            javascript += 1;
        }
        if line.contains("function") {
            if line.ends_with('{') {
                javascript += 1;
            } else if line.ends_with(')') {
                lua += 1;
            }
        }
    }
    match lua.cmp(&javascript) {
        std::cmp::Ordering::Greater => Some(ProfileImportKind::Lua),
        std::cmp::Ordering::Less => Some(ProfileImportKind::JavaScript),
        std::cmp::Ordering::Equal => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "proxies:\n  - name: a\n    type: ss\nrules:\n  - MATCH,DIRECT\n";
    const OVERLAY: &str = "prepend-rules:\n  - DOMAIN,example.com,DIRECT\n";
    const OVERLAY_WITH_PROXIES: &str = "append-proxies:\n  - name: b\nproxies:\n  - name: a\n";
    const JAVASCRIPT: &str =
        "// rename\nfunction main(config) {\n  const n = 1\n  return config\n}\n";
    const LUA: &str = "-- rename\nfunction main(config)\n  local n = 1\n  if n ~= 0 then\n    return config\n  end\nend\n";

    fn facts(content: &str) -> ImportFacts<'_> {
        ImportFacts {
            content,
            url_path: "/sub",
            content_type: None,
            file_name: None,
            has_quota: false,
            has_title: false,
            has_update_interval: false,
        }
    }

    fn signals(classification: &ImportClassification) -> Vec<(ProfileImportKind, ImportSignal)> {
        classification
            .evidence
            .iter()
            .map(|evidence| (evidence.kind, evidence.signal))
            .collect()
    }

    #[test]
    fn a_mapping_with_proxies_is_a_config() {
        let classification = classify(&facts(CONFIG));
        assert_eq!(classification.suggested, Some(ProfileImportKind::Config));
        assert_eq!(
            signals(&classification),
            [(ProfileImportKind::Config, ImportSignal::ProxiesKey)]
        );
    }

    #[test]
    fn a_mapping_with_directives_is_an_overlay() {
        let classification = classify(&facts(OVERLAY));
        assert_eq!(classification.suggested, Some(ProfileImportKind::Overlay));
        assert_eq!(
            signals(&classification),
            [(ProfileImportKind::Overlay, ImportSignal::OverlayDirectives)]
        );
    }

    #[test]
    fn directives_beside_proxies_make_an_overlay_without_subscription_headers() {
        let classification = classify(&facts(OVERLAY_WITH_PROXIES));
        assert_eq!(classification.suggested, Some(ProfileImportKind::Overlay));
    }

    #[test]
    fn a_quota_makes_a_mapping_with_proxies_a_config_despite_directives() {
        let classification = classify(&ImportFacts {
            has_quota: true,
            ..facts(OVERLAY_WITH_PROXIES)
        });
        assert_eq!(classification.suggested, Some(ProfileImportKind::Config));
        assert!(
            signals(&classification)
                .contains(&(ProfileImportKind::Config, ImportSignal::SubscriptionQuota))
        );
    }

    #[test]
    fn subscription_headers_are_evidence_for_a_config() {
        let classification = classify(&ImportFacts {
            has_quota: true,
            has_title: true,
            has_update_interval: true,
            ..facts(CONFIG)
        });
        assert_eq!(
            signals(&classification),
            [
                (ProfileImportKind::Config, ImportSignal::SubscriptionQuota),
                (ProfileImportKind::Config, ImportSignal::ProfileTitle),
                (ProfileImportKind::Config, ImportSignal::UpdateInterval),
                (ProfileImportKind::Config, ImportSignal::ProxiesKey),
            ]
        );
    }

    #[test]
    fn any_other_mapping_is_an_overlay() {
        let classification = classify(&facts("dns:\n  enable: true\n"));
        assert_eq!(classification.suggested, Some(ProfileImportKind::Overlay));
        assert!(classification.evidence.is_empty());
    }

    #[test]
    fn script_syntax_tells_javascript_from_lua() {
        assert_eq!(
            classify(&facts(JAVASCRIPT)).suggested,
            Some(ProfileImportKind::JavaScript)
        );
        assert_eq!(
            classify(&facts(LUA)).suggested,
            Some(ProfileImportKind::Lua)
        );
    }

    #[test]
    fn an_extension_decides_a_script_before_its_syntax() {
        let classification = classify(&ImportFacts {
            url_path: "/scripts/rename.LUA",
            ..facts("function main(config) return config end")
        });
        assert_eq!(classification.suggested, Some(ProfileImportKind::Lua));
        assert_eq!(
            signals(&classification),
            [(ProfileImportKind::Lua, ImportSignal::FileExtension)]
        );
    }

    #[test]
    fn a_content_disposition_name_wins_over_the_url_path() {
        let classification = classify(&ImportFacts {
            url_path: "/download.lua",
            file_name: Some("rename.js"),
            ..facts("")
        });
        assert_eq!(
            classification.suggested,
            Some(ProfileImportKind::JavaScript)
        );
    }

    #[test]
    fn a_media_type_decides_a_script() {
        let classification = classify(&ImportFacts {
            content_type: Some("text/javascript"),
            ..facts("")
        });
        assert_eq!(
            classification.suggested,
            Some(ProfileImportKind::JavaScript)
        );
    }

    #[test]
    fn clash_yaml_wins_over_a_script_extension() {
        let classification = classify(&ImportFacts {
            url_path: "/config.js",
            ..facts(CONFIG)
        });
        assert_eq!(classification.suggested, Some(ProfileImportKind::Config));
    }

    #[test]
    fn unrecognized_content_has_no_suggestion() {
        let classification = classify(&facts("c3M6Ly9ZV1Z6TFRJMU5pMW5ZMjA2Y0dGemMzZHZjbVE"));
        assert_eq!(classification.suggested, None);
        assert!(classification.evidence.is_empty());
    }

    #[test]
    fn a_script_without_distinctive_lines_has_no_suggestion() {
        assert_eq!(classify(&facts("main(config)")).suggested, None);
    }
}
