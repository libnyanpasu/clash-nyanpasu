//! Updater feed calculation from an explicit release channel and optional SourceForge project.

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("invalid SourceForge project slug: {project}"))]
    ValidateSourceforgeProject { project: String },
}
use nyanpasu_config::application::ReleaseChannel;

pub(crate) fn update_endpoints(channel: ReleaseChannel, project: Option<&str>) -> Vec<String> {
    match update_endpoints_for_project(channel, project) {
        Ok(endpoints) => endpoints,
        Err(error) => {
            tracing::warn!(%error, "ignoring invalid SourceForge updater manifest project");
            update_endpoints_for_project(channel, None)
                .expect("the updater endpoints without SourceForge are valid")
        }
    }
}

pub(crate) fn update_endpoints_for_project(
    channel: ReleaseChannel,
    project: Option<&str>,
) -> Result<Vec<String>, Error> {
    let suffix = match channel {
        ReleaseChannel::Stable => "",
        ReleaseChannel::Beta => "-beta",
        ReleaseChannel::Nightly => "-nightly",
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
            return ValidateSourceforgeProjectSnafu { project }.fail();
        }
        endpoints.push(format!(
            "https://{project}.sourceforge.io/updater/update{suffix}.json"
        ));
    }
    Ok(endpoints)
}

pub(crate) fn is_valid_sourceforge_project(project: &str) -> bool {
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
