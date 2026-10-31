//! YAML file reading and merge-key expansion.

use std::path::{Path, PathBuf};

use fs_err as fs;
use serde::de::DeserializeOwned;
use serde_yaml_ng::{Mapping, Value};

#[derive(Debug, snafu::Snafu)]
pub enum YamlError {
    #[snafu(display("file not found {}", path.display()))]
    FindFile { path: PathBuf },
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to parse {}", path.display()))]
    ParseFile {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },
    #[snafu(display("failed to apply merge in {}", path.display()))]
    ApplyMerge {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },
    #[snafu(display("expected a YAML mapping in {}", path.display()))]
    ReadMapping { path: PathBuf },
}

pub fn read_yaml<T: DeserializeOwned, P: AsRef<Path>>(
    path: P,
) -> std::result::Result<T, YamlError> {
    use snafu::ResultExt as _;
    let path = path.as_ref();
    if !path.exists() {
        return FindFileSnafu { path }.fail();
    }
    let yaml_str = fs::read_to_string(path).context(ReadFileSnafu { path })?;
    serde_yaml_ng::from_str(&yaml_str).context(ParseFileSnafu { path })
}

pub fn read_merge_mapping(path: &PathBuf) -> std::result::Result<Mapping, YamlError> {
    use snafu::{OptionExt as _, ResultExt as _};
    let mut val: Value = read_yaml(path)?;
    val.apply_merge().context(ApplyMergeSnafu { path })?;
    Ok(val
        .as_mapping()
        .context(ReadMappingSnafu { path })?
        .to_owned())
}
