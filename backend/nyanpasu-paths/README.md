# nyanpasu-paths

Frontend-independent application directory and binary resolution.

`PathResolver` owns host inputs and platform policy. Constructing it does not resolve or create config/data directories. Frontends supply their application identity, executable discovery result, package portability flag and development binary candidates. The crate owns OS directory selection, platform directory-name casing, Windows registry/SID handling, filesystem preparation, and binary search.

- `new(HostInputs)` uses native directory policy; executable discovery failures remain independent from operations that only need config/data directories.
- `with_base_dirs` supplies explicit roots without requiring executable or bundle resources. `with_roots` binds explicit roots to an existing host instance.
- `with_install_dir` supplies an installation root without executable discovery.
- `config_dir` / `data_dir` preserve base-directory creation and errors; `resolve_data_dir` only resolves a location.
- `resolve_paths(resources)` explicitly prepares config then data and produces a `ResolvedPaths` fixed-root snapshot for application owners. Its leaf paths are pure joins. `ResolvedPaths::with_base_dirs` constructs such values without IO.
- Binary lookup remains dynamic: data → install → supplied development candidate. Diagnostics retain the original directory-valued data hit and unchecked install fallback; the two operations intentionally are not merged.
- Service preparation returns OS identity/directory values, preserving user → data → config → install ordering. Command arguments, elevation and service protocols stay with their consumers.

There is no global resolver, GUI dependency, frontend callback, process owner or service locator. Tauri resource discovery, packaged portability-marker lifetime, and development sidecar naming belong to the GUI composition boundary.

Original value-path and Windows tests moved with the capability. The original config-derived test containing the GUI tray-icon assertion lives in the GUI's tray-icon module. No additional migration tests were introduced.
