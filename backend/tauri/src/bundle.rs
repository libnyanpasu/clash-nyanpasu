use crate::client::app_update::endpoints;

use anyhow::{Result, bail};
use nyanpasu_config::application::{ReleaseChannel, UpdateSource};
use nyanpasu_core::client::bundle_metadata::BundleMetadata;
use std::path::{Path, PathBuf};
use tauri::utils::config::{Config, WebviewInstallMode};

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(context(false), display("failed to parse build version"))]
    ParseVersion { source: semver::Error },
    #[snafu(display("failed to inspect bundled WebView2 directory {}", path.display()))]
    InspectWebview {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("fixed WebView2 runtime is incomplete in {}", path.display()))]
    ValidateWebview { path: PathBuf },
    #[snafu(display("missing updater configuration"))]
    LocateUpdaterConfiguration,
    #[snafu(display("unsupported updater target"))]
    ResolveUpdaterTarget,
    #[snafu(context(false), display("failed to serialize updater endpoints"))]
    SerializeUpdaterEndpoints { source: serde_json::Error },
}

pub fn resolve(
    windows: bool,
    config: &mut Config,
    executable_dir: &Path,
    system_webview_available: impl FnOnce() -> bool,
) -> std::result::Result<BundleMetadata, Error> {
    use snafu::ResultExt as _;
    let metadata = compiled_metadata()?;
    if !windows {
        return Ok(metadata);
    }

    let runtime_dir = fixed_runtime_dir(config, executable_dir);
    let is_fixed_webview = matches!(
        config.bundle.windows.webview_install_mode,
        WebviewInstallMode::FixedRuntime { .. }
    ) || runtime_dir
        .try_exists()
        .context(InspectWebviewSnafu { path: &runtime_dir })?;
    let is_portable = is_portable(executable_dir);
    if is_fixed_webview {
        if runtime_dir.join("msedgewebview2.exe").is_file() {
            config.bundle.windows.webview_install_mode =
                WebviewInstallMode::FixedRuntime { path: runtime_dir };
        } else if !is_portable && system_webview_available() {
            tracing::warn!(path = %runtime_dir.display(), "fixed WebView2 incomplete; using system WebView2; reinstall to restore the bundled runtime");
            config.bundle.windows.webview_install_mode = WebviewInstallMode::default();
        } else {
            return ValidateWebviewSnafu { path: runtime_dir }.fail();
        }
    }

    Ok(BundleMetadata {
        is_portable,
        is_fixed_webview,
        ..metadata
    })
}

pub fn compiled_metadata() -> std::result::Result<BundleMetadata, Error> {
    Ok(BundleMetadata {
        app_version: env!("NYANPASU_VERSION"),
        package_version: env!("CARGO_PKG_VERSION"),
        build_date: crate::consts::BUILD_INFO.build_date,
        is_appimage: *crate::consts::IS_APPIMAGE,
        main_window_label: crate::consts::MAIN_WINDOW_LABEL,
        is_portable: false,
        is_fixed_webview: false,
        release_channel: compiled_channel(cfg!(feature = "nightly"), env!("NYANPASU_VERSION"))?,
        sourceforge_project: option_env!("NYANPASU_SOURCEFORGE_PROJECT"),
    })
}

pub fn setup(
    metadata: &BundleMetadata,
    config: &mut Config,
) -> std::result::Result<tauri_plugin_updater::Builder, Error> {
    use snafu::OptionExt as _;
    config
        .plugins
        .0
        .get_mut("updater")
        .context(LocateUpdaterConfigurationSnafu)?["endpoints"] = serde_json::to_value(
        endpoints::update_endpoints(metadata.release_channel, metadata.sourceforge_project),
    )?;
    let mut updater = tauri_plugin_updater::Builder::new();
    if metadata.is_fixed_webview {
        let target = tauri_plugin_updater::target().context(ResolveUpdaterTargetSnafu)?;
        updater = updater.target(updater_target(metadata, &target));
    }
    Ok(updater)
}

fn updater_target(metadata: &BundleMetadata, target: &str) -> String {
    if metadata.is_fixed_webview {
        format!("{target}-fixed-webview")
    } else {
        target.to_owned()
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

fn compiled_channel(nightly: bool, version: &str) -> std::result::Result<ReleaseChannel, Error> {
    if nightly {
        Ok(ReleaseChannel::Nightly)
    } else if semver::Version::parse(version)?.pre.is_empty() {
        Ok(ReleaseChannel::Stable)
    } else {
        Ok(ReleaseChannel::Beta)
    }
}

pub fn update_endpoints(channel: ReleaseChannel) -> Vec<String> {
    endpoints::update_endpoints(channel, option_env!("NYANPASU_SOURCEFORGE_PROJECT"))
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
    use anyhow::Context as _;
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
    let project_is_canonical = endpoints::is_valid_sourceforge_project(&project);
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

fn decode_path_component(segment: &str, description: &str) -> Result<String> {
    use anyhow::Context as _;
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
    channel: ReleaseChannel,
    local: &semver::Version,
    remote: &tauri_plugin_updater::RemoteRelease,
    build_time: time::OffsetDateTime,
) -> bool {
    use std::cmp::Ordering;
    if channel == ReleaseChannel::Stable && !remote.version.pre.is_empty() {
        return false;
    }
    match local.cmp_precedence(&remote.version) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => {
            channel == ReleaseChannel::Nightly
                && !local.build.is_empty()
                && !remote.version.build.is_empty()
                && local.build != remote.version.build
                && remote.pub_date.is_none_or(|date| date > build_time)
        }
    }
}

#[cfg(test)]
mod tests;
