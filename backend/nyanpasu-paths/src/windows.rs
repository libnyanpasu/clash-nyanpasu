use crate::{
    DiscoverError, HostInputs, Root, error::discover_error::PortableInstallDirSnafu, registry,
    suggested,
};
use camino::Utf8PathBuf;
use snafu::ResultExt as _;

/// Portable layout, then the registry's custom config dir, then the OS defaults. A registry
/// that cannot be read counts as no custom dir, and the data dir never uses it.
pub(super) fn base_dirs(inputs: &HostInputs) -> Result<(Utf8PathBuf, Utf8PathBuf), DiscoverError> {
    if inputs.portable {
        let install_dir = inputs
            .install_dir
            .as_deref()
            .map_err(Clone::clone)
            .context(PortableInstallDirSnafu)?;
        return Ok((
            install_dir.join(".config").join(&inputs.app_name),
            install_dir.join(".data").join(&inputs.app_name),
        ));
    }
    let name = directory_name(&inputs.app_name);
    let config_dir = match registry::custom_config_dir(&inputs.app_name).ok().flatten() {
        Some(dir) => dir,
        None => suggested(
            Root::Config,
            nyanpasu_utils::dirs::suggest_config_dir(&name),
        )?,
    };
    Ok((
        config_dir,
        suggested(Root::Data, nyanpasu_utils::dirs::suggest_data_dir(&name))?,
    ))
}

pub(super) fn executable_name(name: &str) -> String {
    if name.ends_with(".exe") {
        name.to_owned()
    } else {
        format!("{name}.exe")
    }
}

pub(super) fn directory_name(app_name: &str) -> String {
    use convert_case::{Case, Casing};
    app_name.to_case(Case::Title)
}

#[cfg(test)]
mod tests {
    use crate::{DiscoverError, HostInputs, InstallDirError, PathResolver};
    use camino::Utf8PathBuf;
    use std::collections::BTreeMap;

    fn inputs(install_dir: Result<Utf8PathBuf, InstallDirError>) -> HostInputs {
        HostInputs {
            app_name: "clash-nyanpasu".into(),
            install_dir,
            portable: true,
            development_binaries: BTreeMap::new(),
        }
    }

    #[test]
    fn the_portable_layout_keeps_config_and_data_beside_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let install = Utf8PathBuf::from_path_buf(root.path().to_owned()).unwrap();

        let resolver = PathResolver::discover(inputs(Ok(install.clone()))).unwrap();

        assert_eq!(
            resolver.app_config_dir(),
            install.join(".config").join("clash-nyanpasu")
        );
        assert_eq!(
            resolver.app_data_dir(),
            install.join(".data").join("clash-nyanpasu")
        );
        assert_eq!(resolver.app_install_dir().unwrap(), install);
    }

    #[test]
    fn the_portable_layout_needs_the_install_dir() {
        let error = PathResolver::discover(inputs(Err(InstallDirError::NoParent))).unwrap_err();

        assert!(matches!(error, DiscoverError::PortableInstallDir { .. }));
    }
}
