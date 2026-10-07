use crate::{DiscoverError, HostInputs, Root, suggested};
use camino::Utf8PathBuf;

/// The OS defaults. There is no portable layout off Windows.
pub(super) fn base_dirs(inputs: &HostInputs) -> Result<(Utf8PathBuf, Utf8PathBuf), DiscoverError> {
    let name = directory_name(&inputs.app_name);
    Ok((
        suggested(
            Root::Config,
            nyanpasu_utils::dirs::suggest_config_dir(&name),
        )?,
        suggested(Root::Data, nyanpasu_utils::dirs::suggest_data_dir(&name))?,
    ))
}

pub(super) fn executable_name(name: &str) -> String {
    name.to_owned()
}

pub(super) fn directory_name(app_name: &str) -> String {
    use convert_case::{Case, Casing};
    if cfg!(target_os = "macos") {
        app_name.to_case(Case::Title)
    } else {
        app_name.to_owned()
    }
}
