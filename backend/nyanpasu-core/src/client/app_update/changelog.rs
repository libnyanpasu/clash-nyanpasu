//! Release notes between two application versions.
//!
//! `CHANGELOG.md` on the default branch holds one section per tag, so the
//! sections after the installed version up to the offered one describe the
//! update without overlap. GitHub releases fill in what the file lacks, and the
//! compare link stays as the last resort.

use std::cmp::Ordering;

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use nyanpasu_config::application::UpdateSource;
use semver::Version;
use serde::{Deserialize, Serialize};
use specta::Type;

const REPOSITORY_URL: &str = "https://github.com/libnyanpasu/clash-nyanpasu";
const CHANGELOG_URL: &str =
    "https://raw.githubusercontent.com/libnyanpasu/clash-nyanpasu/main/CHANGELOG.md";
const RELEASES_URL: &str =
    "https://api.github.com/repos/libnyanpasu/clash-nyanpasu/releases?per_page=100";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdateChangelogSource {
    Changelog,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppUpdateChangelogEntry {
    pub version: String,
    pub date: Option<String>,
    pub body: String,
    pub source: AppUpdateChangelogSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppUpdateChangelog {
    /// Newest first.
    pub entries: Vec<AppUpdateChangelogEntry>,
    /// `None` when either version is a nightly build, which has no tag.
    pub compare_url: Option<String>,
}

/// Fetches a text document; the adapter owns proxies and network deadlines.
#[async_trait]
pub trait ChangelogHttp: Send + Sync + 'static {
    async fn get_text(&self, url: &url::Url) -> Result<String>;
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
}

struct Section {
    version: Version,
    entry: AppUpdateChangelogEntry,
}

/// Collects the notes of every version in `(from, to]`.
pub async fn resolve_changelog(
    http: &dyn ChangelogHttp,
    sources: &[UpdateSource],
    from: &str,
    to: &str,
) -> Result<AppUpdateChangelog> {
    let from = parse_version(from)?;
    let to = parse_version(to)?;
    if to.cmp_precedence(&from) != Ordering::Greater {
        bail!("{to} is not newer than {from}");
    }
    // Nightly builds carry build metadata and are not tagged.
    if !from.build.is_empty() || !to.build.is_empty() {
        return Ok(AppUpdateChangelog {
            entries: Vec::new(),
            compare_url: None,
        });
    }

    let in_range = |version: &Version| {
        version.cmp_precedence(&from) == Ordering::Greater
            && version.cmp_precedence(&to) != Ordering::Greater
    };

    let mut sections = Vec::new();
    let mut changelog_complete = false;
    for url in changelog_urls(sources) {
        match http.get_text(&url).await {
            Ok(markdown) => {
                sections = parse_changelog(&markdown);
                sections.retain(|section| in_range(&section.version));
                changelog_complete = sections.iter().any(|section| section.version == to);
                break;
            }
            Err(error) => tracing::warn!(%url, %error, "failed to fetch CHANGELOG.md"),
        }
    }

    if !changelog_complete {
        match fetch_releases(http).await {
            Ok(releases) => {
                let mut missing = releases
                    .into_iter()
                    .filter(|release| {
                        in_range(&release.version)
                            && !sections
                                .iter()
                                .any(|known| known.version == release.version)
                    })
                    .collect::<Vec<_>>();
                drop_folded_prereleases(&mut missing);
                sections.extend(missing);
            }
            Err(error) => tracing::warn!(%error, "failed to fetch GitHub releases"),
        }
    }

    sections.sort_by(|a, b| b.version.cmp_precedence(&a.version));
    Ok(AppUpdateChangelog {
        entries: sections.into_iter().map(|section| section.entry).collect(),
        compare_url: Some(format!("{REPOSITORY_URL}/compare/v{from}...v{to}")),
    })
}

fn parse_version(version: &str) -> Result<Version> {
    let version = version.trim();
    Version::parse(version.strip_prefix('v').unwrap_or(version))
        .with_context(|| format!("invalid version: {version}"))
}

/// The configured GitHub-backed sources in order; the other mirrors only serve
/// release assets.
fn changelog_urls(sources: &[UpdateSource]) -> Vec<url::Url> {
    let mut urls = Vec::new();
    for source in sources {
        let url = match source {
            UpdateSource::Github => CHANGELOG_URL.to_string(),
            UpdateSource::Ghfast => format!("https://ghfast.top/{CHANGELOG_URL}"),
            UpdateSource::Nyanpasu | UpdateSource::Sourceforge => continue,
        };
        let url = url::Url::parse(&url).expect("changelog URLs are valid");
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    if urls.is_empty() {
        urls.push(url::Url::parse(CHANGELOG_URL).expect("changelog URL is valid"));
    }
    urls
}

/// Splits a git-cliff changelog at its `## [version] - date` headings.
fn parse_changelog(markdown: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current: Option<(Version, Option<String>, Vec<&str>)> = None;
    for line in markdown.lines() {
        if let Some((version, date)) = parse_section_heading(line) {
            sections.extend(current.take().map(finish_changelog_section));
            current = Some((version, date, Vec::new()));
        } else if let Some((_, _, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    sections.extend(current.map(finish_changelog_section));
    sections
}

fn finish_changelog_section(
    (version, date, lines): (Version, Option<String>, Vec<&str>),
) -> Section {
    Section {
        entry: AppUpdateChangelogEntry {
            version: version.to_string(),
            date,
            body: tidy_body(&lines.join("\n")),
            source: AppUpdateChangelogSource::Changelog,
        },
        version,
    }
}

fn parse_section_heading(line: &str) -> Option<(Version, Option<String>)> {
    let rest = line.strip_prefix("## [")?;
    let (version, rest) = rest.split_once(']')?;
    let version = Version::parse(version).ok()?;
    let date = rest
        .trim()
        .strip_prefix('-')
        .map(str::trim)
        .filter(|date| !date.is_empty())
        .map(str::to_string);
    Some((version, date))
}

async fn fetch_releases(http: &dyn ChangelogHttp) -> Result<Vec<Section>> {
    let url = url::Url::parse(RELEASES_URL).expect("releases URL is valid");
    let releases: Vec<GithubRelease> = serde_json::from_str(&http.get_text(&url).await?)?;
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version = parse_version(&release.tag_name).ok()?;
            let body = release.body.unwrap_or_default();
            // git-cliff release bodies open with the same heading the entry shows.
            let body = match body.trim_start().split_once('\n') {
                Some((first, rest)) if parse_section_heading(first.trim_end()).is_some() => rest,
                _ => body.as_str(),
            };
            Some(Section {
                entry: AppUpdateChangelogEntry {
                    version: version.to_string(),
                    date: release
                        .published_at
                        .map(|date| date.split('T').next().unwrap_or(&date).to_string()),
                    body: tidy_body(body),
                    source: AppUpdateChangelogSource::Release,
                },
                version,
            })
        })
        .collect())
}

/// A stable release body folds in its prereleases, so listing those as well
/// would repeat their changes.
fn drop_folded_prereleases(releases: &mut Vec<Section>) {
    let stable = releases
        .iter()
        .filter(|release| release.version.pre.is_empty())
        .map(|release| {
            (
                release.version.major,
                release.version.minor,
                release.version.patch,
            )
        })
        .collect::<Vec<_>>();
    releases.retain(|release| {
        let version = &release.version;
        version.pre.is_empty() || !stable.contains(&(version.major, version.minor, version.patch))
    });
}

/// Drops the per-section compare footer, which the combined compare link
/// replaces, and nests the remaining headings under the version heading.
fn tidy_body(body: &str) -> String {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if in_fence {
            lines.push(line.to_string());
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("**Full Changelog**")
            || (trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-'))
        {
            continue;
        }
        if lines
            .last()
            .is_some_and(|last: &String| last.trim().is_empty())
            && trimmed.is_empty()
        {
            continue;
        }
        match line.strip_prefix("## ") {
            Some(heading) => lines.push(format!("### {heading}")),
            None => lines.push(line.to_string()),
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const CHANGELOG: &str = "\
## [2.0.0] - 2026-11-01

### ✨ Features

- **ui:** Stable feature by @keiko233 in [#3](https://github.com/libnyanpasu/clash-nyanpasu/pull/3)

-----------------

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-rc.2...v2.0.0

## [2.0.0-rc.2] - 2026-10-14

### 🐛 Bug Fixes

- rc.2 fix by @greenhat616

## New Contributors
* @someone made their first contribution

## [2.0.0-rc.1] - 2026-10-09

- rc.1 change

## [1.6.1] - 2024-09-07

- old change
";

    #[derive(Default)]
    struct FakeHttp(HashMap<String, String>);

    impl FakeHttp {
        fn with(mut self, url: &str, body: &str) -> Self {
            self.0.insert(url.to_string(), body.to_string());
            self
        }
    }

    #[async_trait]
    impl ChangelogHttp for FakeHttp {
        async fn get_text(&self, url: &url::Url) -> Result<String> {
            self.0
                .get(url.as_str())
                .cloned()
                .with_context(|| format!("404 {url}"))
        }
    }

    fn versions(changelog: &AppUpdateChangelog) -> Vec<(&str, AppUpdateChangelogSource)> {
        changelog
            .entries
            .iter()
            .map(|entry| (entry.version.as_str(), entry.source))
            .collect()
    }

    fn releases_json(releases: &[(&str, &str, bool)]) -> String {
        serde_json::to_string(
            &releases
                .iter()
                .map(|(tag, body, draft)| {
                    serde_json::json!({
                        "tag_name": tag,
                        "body": body,
                        "published_at": "2026-10-14T08:00:00Z",
                        "draft": draft,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn changelog_sections_after_the_installed_version_up_to_the_target() {
        let http = FakeHttp::default().with(CHANGELOG_URL, CHANGELOG);
        let changelog = resolve_changelog(&http, &[UpdateSource::Github], "2.0.0-rc.1", "v2.0.0")
            .await
            .unwrap();
        assert_eq!(
            versions(&changelog),
            [
                ("2.0.0", AppUpdateChangelogSource::Changelog),
                ("2.0.0-rc.2", AppUpdateChangelogSource::Changelog),
            ]
        );
        assert_eq!(changelog.entries[0].date.as_deref(), Some("2026-11-01"));
        assert_eq!(
            changelog.compare_url.as_deref(),
            Some("https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-rc.1...v2.0.0")
        );
    }

    #[tokio::test]
    async fn a_stable_upgrade_lists_the_prereleases_before_it() {
        let http = FakeHttp::default().with(CHANGELOG_URL, CHANGELOG);
        let changelog = resolve_changelog(&http, &[UpdateSource::Github], "1.6.1", "2.0.0")
            .await
            .unwrap();
        assert_eq!(
            changelog
                .entries
                .iter()
                .map(|entry| entry.version.as_str())
                .collect::<Vec<_>>(),
            ["2.0.0", "2.0.0-rc.2", "2.0.0-rc.1"]
        );
    }

    #[tokio::test]
    async fn bodies_drop_compare_footers_and_nest_headings() {
        let http = FakeHttp::default().with(CHANGELOG_URL, CHANGELOG);
        let changelog = resolve_changelog(&http, &[UpdateSource::Github], "2.0.0-rc.1", "2.0.0")
            .await
            .unwrap();
        let stable = &changelog.entries[0].body;
        assert!(!stable.contains("Full Changelog"));
        assert!(!stable.contains("-----"));
        assert!(stable.ends_with("pull/3)"));
        assert!(changelog.entries[1].body.contains("### New Contributors"));
    }

    #[tokio::test]
    async fn ghfast_is_tried_after_a_failed_source_in_configured_order() {
        let http =
            FakeHttp::default().with(&format!("https://ghfast.top/{CHANGELOG_URL}"), CHANGELOG);
        let changelog = resolve_changelog(
            &http,
            &[
                UpdateSource::Nyanpasu,
                UpdateSource::Github,
                UpdateSource::Ghfast,
            ],
            "2.0.0-rc.2",
            "2.0.0",
        )
        .await
        .unwrap();
        assert_eq!(
            versions(&changelog),
            [("2.0.0", AppUpdateChangelogSource::Changelog)]
        );
    }

    #[tokio::test]
    async fn releases_fill_versions_the_changelog_lacks() {
        let releases = releases_json(&[
            ("v2.0.1", "## [2.0.1] - 2026-11-02\n\n- patch fix", false),
            ("v2.0.2", "- draft", true),
        ]);
        let http = FakeHttp::default()
            .with(CHANGELOG_URL, CHANGELOG)
            .with(RELEASES_URL, &releases);
        let changelog = resolve_changelog(&http, &[UpdateSource::Github], "2.0.0-rc.2", "2.0.1")
            .await
            .unwrap();
        assert_eq!(
            versions(&changelog),
            [
                ("2.0.1", AppUpdateChangelogSource::Release),
                ("2.0.0", AppUpdateChangelogSource::Changelog),
            ]
        );
        assert_eq!(changelog.entries[0].body, "- patch fix");
        assert_eq!(changelog.entries[0].date.as_deref(), Some("2026-10-14"));
    }

    #[tokio::test]
    async fn release_fallback_skips_prereleases_folded_into_a_stable_body() {
        let releases = releases_json(&[
            ("v2.1.0", "- everything", false),
            ("v2.1.0-rc.1", "- rc", false),
            ("v2.1.1-beta.1", "- next beta", false),
        ]);
        let http = FakeHttp::default().with(RELEASES_URL, &releases);
        let changelog = resolve_changelog(&http, &[UpdateSource::Github], "2.0.0", "2.1.1-beta.1")
            .await
            .unwrap();
        assert_eq!(
            versions(&changelog),
            [
                ("2.1.1-beta.1", AppUpdateChangelogSource::Release),
                ("2.1.0", AppUpdateChangelogSource::Release),
            ]
        );
    }

    #[tokio::test]
    async fn without_any_notes_only_the_compare_link_remains() {
        let changelog = resolve_changelog(&FakeHttp::default(), &[], "2.0.0-rc.1", "2.0.0")
            .await
            .unwrap();
        assert!(changelog.entries.is_empty());
        assert!(changelog.compare_url.is_some());
    }

    #[tokio::test]
    async fn nightly_versions_have_no_tagged_notes() {
        let http = FakeHttp::default().with(CHANGELOG_URL, CHANGELOG);
        let changelog = resolve_changelog(
            &http,
            &[UpdateSource::Github],
            "2.0.0-rc.2",
            "2.0.0-alpha.1+abcdef0",
        )
        .await;
        assert!(changelog.is_err(), "an older nightly is not an update");
        let changelog = resolve_changelog(
            &http,
            &[UpdateSource::Github],
            "2.0.0-rc.2",
            "2.0.1-alpha+abcdef0",
        )
        .await
        .unwrap();
        assert_eq!(
            changelog,
            AppUpdateChangelog {
                entries: Vec::new(),
                compare_url: None,
            }
        );
    }

    #[tokio::test]
    async fn the_target_must_be_newer() {
        let http = FakeHttp::default().with(CHANGELOG_URL, CHANGELOG);
        assert!(
            resolve_changelog(&http, &[], "2.0.0", "2.0.0")
                .await
                .is_err()
        );
        assert!(
            resolve_changelog(&http, &[], "2.0.0", "1.6.1")
                .await
                .is_err()
        );
        assert!(
            resolve_changelog(&http, &[], "2.0.0", "latest")
                .await
                .is_err()
        );
    }

    #[test]
    fn code_fences_keep_their_contents() {
        assert_eq!(
            tidy_body("```md\n## heading\n---\n```\n## Notes"),
            "```md\n## heading\n---\n```\n### Notes"
        );
    }
}
