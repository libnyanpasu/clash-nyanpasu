//! Temporary path resolvers shared by tests of independent backend capabilities.

use camino::Utf8PathBuf;
use nyanpasu_paths::PathResolver;

/// A resolver rooted at `config` and `data`, for tests that do not run from an installed
/// executable.
pub(crate) fn test_paths(
    config: impl AsRef<std::path::Path>,
    data: impl AsRef<std::path::Path>,
) -> PathResolver {
    let utf8 = |path: &std::path::Path| {
        Utf8PathBuf::from_path_buf(path.to_owned()).expect("test directories are UTF-8")
    };
    PathResolver::with_base_dirs(utf8(config.as_ref()), utf8(data.as_ref()))
}
