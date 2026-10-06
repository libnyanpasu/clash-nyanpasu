# nyanpasu-paths

Frontend-independent application directory and binary resolution.

`PathResolver` is the one path value an application passes around. It holds the resolved config and data roots and the install-dir outcome; every other path is a pure function of that context, and cloning shares it. Frontends supply their application identity, the executable's directory, a portable flag and development binary candidates as `HostInputs`.

- `PathResolver::discover(HostInputs)` is the only discovery step: OS directory suggestions for config and data, plus on Windows the portable layout and the registry's custom config dir. It creates nothing and fails with a `DiscoverError` when a root has no platform default, is not valid UTF-8, or a portable layout has no install dir.
- Windows config priority is portable layout, then the registry's custom config dir (a registry that cannot be read counts as unset), then the OS default; the data dir never uses the registry. Directory names are title-cased on Windows and macOS.
- `with_base_dirs` supplies explicit roots without a host, for tests and tools; its install dir is `InstallDirError::Unknown`.
- Roots are borrowed from the context as `&Utf8Path`. Paths derived from them (profiles, config files, storage, logs, cache, backups) are pure joins returning `Utf8PathBuf`. A path that is not UTF-8 is rejected once, in `discover` and `installation_dir`, so later joins cannot fail.
- `create_base_dirs` is the explicit IO step: the config dir, then the data dir, both propagating errors, then the profiles, logs and cache dirs best-effort. `create_dir_all` replaces a file standing where a directory belongs.
- Binary lookup stays dynamic and uncached, because a binary may be downloaded after startup. `find_binary_path` searches the data dir, the install dir, then the supplied development candidate. `data_or_sidecar_path` returns the data dir itself on a data hit and the install-dir path otherwise; the two operations are intentionally not merged.
- `registry` (Windows) reads and writes the custom config dir the next launch uses. It is persisted for the next start, not a directory of this process, so it is read on demand and never cached.
- Errors are snafu domain errors: `InstallDirError`, `DiscoverError`, `CreateDirError` and, on Windows, `RegistryError`. `InstallDirError` is `Clone` because the resolver stores it.

There is no global resolver, GUI dependency, frontend callback, process owner or service locator. Service identity and command templates, the single-instance name, Tauri resource discovery, the portable marker and development sidecar naming belong to the frontend.
