use anyhow::{Context, Result, bail};
use nyanpasu_config::application::{ReleaseChannel as Channel, UpdateSource};
use std::path::{Path, PathBuf};
use tauri::utils::config::{Config, WebviewInstallMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleMetadata {
    pub is_portable: bool,
    pub is_fixed_webview: bool,
    pub release_channel: Channel,
}

impl BundleMetadata {
    pub fn resolve(windows: bool, config: &Config, executable_dir: &Path) -> Result<Self> {
        let release_channel =
            compiled_channel(cfg!(feature = "nightly"), env!("NYANPASU_VERSION"))?;
        if !windows {
            return Ok(Self {
                release_channel,
                is_portable: false,
                is_fixed_webview: false,
            });
        }

        let runtime_dir = fixed_runtime_dir(config, executable_dir);
        let is_fixed_webview = matches!(
            config.bundle.windows.webview_install_mode,
            WebviewInstallMode::FixedRuntime { .. }
        ) || runtime_dir
            .try_exists()
            .context("failed to inspect the bundled WebView2 directory")?;
        if is_fixed_webview && !runtime_dir.join("msedgewebview2.exe").is_file() {
            bail!(
                "fixed WebView2 runtime is incomplete in {}",
                runtime_dir.display()
            );
        }

        Ok(Self {
            release_channel,
            is_portable: is_portable(executable_dir),
            is_fixed_webview,
        })
    }

    pub fn setup(
        &self,
        config: &mut Config,
        executable_dir: &Path,
    ) -> Result<tauri_plugin_updater::Builder> {
        config
            .plugins
            .0
            .get_mut("updater")
            .context("missing updater configuration")?["endpoints"] =
            serde_json::to_value(update_endpoints(self.release_channel))?;
        let mut updater = tauri_plugin_updater::Builder::new();
        if self.is_fixed_webview {
            let target = tauri_plugin_updater::target().context("unsupported updater target")?;
            updater = updater.target(self.updater_target(&target));
            // Tauri applies this before creating its runtime, including the WebView2 env var.
            config.bundle.windows.webview_install_mode = WebviewInstallMode::FixedRuntime {
                path: fixed_runtime_dir(config, executable_dir),
            };
        }
        Ok(updater)
    }

    fn updater_target(&self, target: &str) -> String {
        if self.is_fixed_webview {
            format!("{target}-fixed-webview")
        } else {
            target.to_owned()
        }
    }
}

pub(super) fn is_portable(executable_dir: &Path) -> bool {
    executable_dir.join(".config/PORTABLE").exists()
}

fn fixed_runtime_dir(config: &Config, executable_dir: &Path) -> PathBuf {
    let path = match &config.bundle.windows.webview_install_mode {
        WebviewInstallMode::FixedRuntime { path } => path.as_path(),
        _ => Path::new("WebView2"),
    };
    executable_dir.join(path)
}

fn compiled_channel(nightly: bool, version: &str) -> Result<Channel> {
    if nightly {
        Ok(Channel::Nightly)
    } else if semver::Version::parse(version)?.pre.is_empty() {
        Ok(Channel::Stable)
    } else {
        Ok(Channel::Beta)
    }
}

pub fn update_endpoints(channel: Channel) -> Vec<String> {
    match update_endpoints_for_project(channel, option_env!("NYANPASU_SOURCEFORGE_PROJECT")) {
        Ok(endpoints) => endpoints,
        Err(error) => {
            tracing::warn!(%error, "ignoring invalid SourceForge updater manifest project");
            update_endpoints_for_project(channel, None)
                .expect("the updater endpoints without SourceForge are valid")
        }
    }
}

fn update_endpoints_for_project(channel: Channel, project: Option<&str>) -> Result<Vec<String>> {
    let suffix = match channel {
        Channel::Stable => "",
        Channel::Beta => "-beta",
        Channel::Nightly => "-nightly",
    };
    let mut endpoints = vec![
        format!(
            "https://nyanpasu-script.majokeiko.com/libnyanpasu/clash-nyanpasu/releases/download/updater/update{suffix}-proxy.json"
        ),
        format!("https://nyanpasu.surge.sh/updater/update{suffix}-proxy.json"),
        format!(
            "https://github.com/libnyanpasu/clash-nyanpasu/releases/download/updater/update{suffix}.json"
        ),
        format!(
            "https://ghfast.top/https://github.com/libnyanpasu/clash-nyanpasu/releases/download/updater/update{suffix}.json"
        ),
    ];
    if let Some(project) = project.filter(|project| !project.trim().is_empty()) {
        if !is_valid_sourceforge_project(project) {
            bail!("invalid SourceForge project slug: {project}");
        }
        endpoints.push(format!(
            "https://{project}.sourceforge.io/updater/update{suffix}.json"
        ));
    }
    Ok(endpoints)
}

pub fn update_download_urls(
    announced: &url::Url,
    sources: &[UpdateSource],
    raw_json: &serde_json::Value,
    target: &str,
) -> Result<Vec<(UpdateSource, url::Url)>> {
    nyanpasu_config::application::validate_update_sources(sources).map_err(anyhow::Error::msg)?;
    if announced.scheme() != "https"
        || !matches!(
            announced.host_str(),
            Some("github.com" | "nyanpasu-script.majokeiko.com")
        )
        || !announced
            .path()
            .starts_with("/libnyanpasu/clash-nyanpasu/releases/download/")
    {
        bail!("unsupported application update download URL: {announced}");
    }
    let mut candidates = Vec::with_capacity(sources.len());
    for source in sources {
        if *source == UpdateSource::Sourceforge {
            match sourceforge_download_url(raw_json, target, announced) {
                Ok(Some(url)) => candidates.push((*source, url)),
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "ignoring invalid SourceForge update mirror"),
            }
            continue;
        }
        let mut url = announced.clone();
        let host = match source {
            UpdateSource::Nyanpasu => "nyanpasu-script.majokeiko.com",
            UpdateSource::Github | UpdateSource::Ghfast => "github.com",
            UpdateSource::Sourceforge => unreachable!(),
        };
        url.set_host(Some(host))
            .expect("update source hosts are valid");
        if *source == UpdateSource::Ghfast {
            url = url::Url::parse(&format!("https://ghfast.top/{url}"))?;
        }
        candidates.push((*source, url));
    }
    if candidates.is_empty() {
        bail!("no configured update package source is available for this release");
    }
    Ok(candidates)
}

fn sourceforge_download_url(
    raw_json: &serde_json::Value,
    target: &str,
    announced: &url::Url,
) -> Result<Option<url::Url>> {
    let Some(platform) = raw_json
        .get("platforms")
        .and_then(|value| value.get(target))
    else {
        return Ok(None);
    };
    let Some(project) = platform.get("project").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(build_id) = platform.get("build_id").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(raw_url) = platform
        .get("mirrors")
        .and_then(|value| value.get("sourceforge"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    let url = url::Url::parse(raw_url).context("invalid SourceForge update URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some("downloads.sourceforge.net")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("unsupported SourceForge update URL: {url}");
    }

    let path = url
        .path_segments()
        .context("SourceForge URL cannot be a base URL")?;
    let segments = path.collect::<Vec<_>>();
    let announced_asset = announced
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .context("announced update URL has no artifact name")?;
    let project = decode_path_component(project, "project")?;
    let build_id = decode_path_component(build_id, "build id")?;
    let actual_project = segments
        .get(1)
        .map(|segment| decode_path_component(segment, "project"))
        .transpose()?;
    let actual_build_id = segments
        .get(3)
        .map(|segment| decode_path_component(segment, "build id"))
        .transpose()?;
    let actual_asset = segments
        .get(4)
        .map(|segment| decode_path_component(segment, "artifact name"))
        .transpose()?;
    let announced_asset = decode_path_component(announced_asset, "artifact name")?;
    let project_is_canonical = is_valid_sourceforge_project(&project);
    let build_id_is_canonical = !build_id.is_empty()
        && !matches!(
            build_id.as_str(),
            "." | ".." | "latest" | "current" | "stable"
        )
        && build_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if !project_is_canonical
        || !build_id_is_canonical
        || segments.len() != 5
        || segments[0] != "project"
        || actual_project.as_deref() != Some(project.as_str())
        || !matches!(segments[2], "nightly" | "releases")
        || actual_build_id.as_deref() != Some(build_id.as_str())
        || actual_asset.as_deref() != Some(announced_asset.as_str())
    {
        bail!("SourceForge update URL does not match its immutable build metadata: {url}");
    }
    Ok(Some(url))
}

fn is_valid_sourceforge_project(project: &str) -> bool {
    !project.is_empty()
        && project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && project.as_bytes()[0].is_ascii_alphanumeric()
        && project
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn decode_path_component(segment: &str, description: &str) -> Result<String> {
    let decoded = percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .with_context(|| format!("invalid UTF-8 in SourceForge {description}"))?
        .into_owned();
    if decoded.is_empty()
        || decoded.contains('/')
        || decoded.contains('\\')
        || decoded.chars().any(char::is_control)
    {
        bail!("invalid SourceForge {description} path component");
    }
    Ok(decoded)
}

pub fn is_newer_release(
    channel: Channel,
    local: &semver::Version,
    remote: &tauri_plugin_updater::RemoteRelease,
    build_time: time::OffsetDateTime,
) -> bool {
    use std::cmp::Ordering;
    if channel == Channel::Stable && !remote.version.pre.is_empty() {
        return false;
    }
    match local.cmp_precedence(&remote.version) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => {
            channel == Channel::Nightly
                && !local.build.is_empty()
                && !remote.version.build.is_empty()
                && local.build != remote.version.build
                && remote.pub_date.is_none_or(|date| date > build_time)
        }
    }
}

#[cfg(test)]
mod tests;
