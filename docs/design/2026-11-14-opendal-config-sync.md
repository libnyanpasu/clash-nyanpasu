# Cross-platform configuration sync over Apache OpenDAL

**Date:** 2026-11-14
**Status:** Proposed; no production code yet
**Baseline:** `main @ 253e2ee6d`
**Scope:** A path manifest that enumerates every syncable item, platform-neutral portable projections of the configuration documents, a provider-agnostic storage port implemented with OpenDAL, an incremental sync protocol, and a conflict protocol surfaced through `NyanpasuClient` and unified RPC.
**Out of scope:** Syncing logs, traffic history, core binaries, geodata or runtime state; real-time collaboration; a hosted sync service; OAuth providers and client-side encryption in the first delivery (both have a reserved place, see [§5.6](#56-credentials) and [§6.9](#69-reserved-client-side-encryption)).

Facts under "verified" were checked against the source at the baseline commit. OpenDAL facts were checked by compiling and running a probe against `opendal 0.59.5` ([Appendix A](#appendix-a-opendal-0595-probe)).

---

## 0. Decisions

1. **Syncability is an allowlist owned by `nyanpasu-paths`.** A `PathManifest` classifies every application path. `SyncLayout` enumerates the syncable items as descriptors with a platform-neutral `LogicalPath`. Nothing is synced because of where it sits; a file that no descriptor names never leaves the device.
2. **The three configuration documents sync as portable projections, not as files.** `application.yaml`, `clash-config.yaml` and `profiles.yaml` each mix portable and device-bound fields. A static field-scope table in `nyanpasu-config` splits them. The projection is what gets hashed, uploaded and merged; device-bound fields never leave the machine. The on-disk documents and their owners keep one file and one atomic commit each ([§11.1](#111-physically-splitting-documents-into-portable-and-local-files) explains why the files are not split).
3. **Remote changes are applied through the domain owners.** The sync owner never writes a configuration file under a running actor. It calls `Replace` on the application, Clash and profiles owners with the version it planned against, so validation, the runtime vote, effects and UI notifications behave as for any other mutation.
4. **OpenDAL stays inside one implementation crate.** `nyanpasu-sync-opendal` implements a byte-level `RemoteStore` port from `nyanpasu-contracts`. It builds operators with typed builders (`Operator::from_iter::<services::Webdav>`), so there is no `ctor` registration and no process-wide registry or HTTP transport.
5. **The remote is an append-only store of content-addressed objects and immutable commit manifests.** No object is ever overwritten, so a torn or lost overwrite cannot corrupt the vault, and eventual consistency only delays visibility.
6. **Correctness does not depend on conditional writes.** Providers that prove create-if-absent in a self-test get linear history; the others (WebDAV, Dropbox, Google Drive) use content-named commits and resolve forks by a three-way merge from the common ancestor.
7. **Conflicts are detected by three-way comparison against a local baseline.** Documents merge per field, profile catalogs per item, files atomically. Policies are `manual` (default), `local_wins`, `remote_wins` and `keep_both`. Every overwritten or deleted local item is preserved in a pre-image snapshot first.
8. **First delivery:** WebDAV, S3-compatible and local filesystem. Sync is an optional capability of a shell; the headless server does not link it until asked.

---

## 1. Current state (verified)

### 1.1 Roots

`PathResolver` (`backend/nyanpasu-paths/src/lib.rs`) holds two roots and derives every other path as a pure join.

| Root         | Linux                           | macOS                           | Windows                                                                 |
| ------------ | ------------------------------- | ------------------------------- | ----------------------------------------------------------------------- |
| `config_dir` | OS config dir, `clash-nyanpasu` | OS config dir, `Clash Nyanpasu` | portable `<install>/.config/<app>`, then registry `AppDir`, then OS dir |
| `data_dir`   | OS data dir, `clash-nyanpasu`   | OS data dir, `Clash Nyanpasu`   | portable `<install>/.data/<app>`, then OS dir (never the registry)      |

The data dir can sit inside the config dir (`backup.rs::skipped_paths` handles that case), so classification must be by entry, not by root.

### 1.2 Inventory

| Path                                                                                                    | Resolver method                                            | Owner                      | Content                                                   | Class today                          |
| ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | -------------------------- | --------------------------------------------------------- | ------------------------------------ |
| `<config>/application.yaml`                                                                             | `application_config_path`                                  | `ApplicationActor`         | `NyanpasuAppConfig`, stamped `_nyanpasu`                  | mixed portable and device            |
| `<config>/clash-config.yaml`                                                                            | `clash_config_path`                                        | `ClashConfigActor`         | `ClashConfig`, stamped                                    | mixed                                |
| `<config>/profiles.yaml`                                                                                | `profiles_path`                                            | profiles actor             | `Profiles` catalog, stamped                               | mixed                                |
| `<config>/profiles/<file>`                                                                              | `app_profiles_dir`                                         | `ProfileFileService`       | configs, overlays, JS/Lua scripts, subscription downloads | mixed by profile source              |
| `<config>/profiles/.profile-materialization-v1/`                                                        | none (profile-io constant)                                 | `ProfileFileService`       | staging, backup, journals, cleanup tombstones             | device, transactional                |
| `<config>/icons/<mode>.png`                                                                             | none (`tray/icon.rs` join)                                 | Tauri tray                 | custom tray icons                                         | portable asset                       |
| `<config>/session-state.yaml`                                                                           | `session_state_path`                                       | `SessionStateActor`        | window geometry                                           | device                               |
| `<config>/migration-state.yaml`                                                                         | none (`store.rs` constant)                                 | migration runner           | module revisions, task records                            | device bookkeeping                   |
| `<config>/runtime/clash-config.yaml`, `candidate*`                                                      | none (`client/runtime.rs`)                                 | runtime publisher          | generated core config                                     | derived                              |
| `<config>/runtime/control/`                                                                             | none (`actor_v2/local_host`)                               | core manager               | control sockets                                           | runtime                              |
| `<config>/nyanpasu-config.yaml`, `clash-guard-overrides.yaml`, `clash.yaml` (previous typed Clash file) | `nyanpasu_config_path`, `clash_guard_overrides_path`, none | migration only             | legacy inputs                                             | legacy                               |
| `<config>/**/.atomicwrite*`                                                                             | none                                                       | `atomicwrites`             | temp dirs of in-flight writes                             | transient                            |
| `<data>/storage.db`                                                                                     | `storage_path`                                             | `Storage` (redb)           | `hotkeys`, `web:*` UI keys (custom CSS, dashboard layout) | device (see [§3.6](#36-later-items)) |
| `<data>/jobs.redb`                                                                                      | `jobs_path`                                                | jobs owner                 | run history                                               | device                               |
| `<data>/backups/`                                                                                       | `backups_dir`                                              | `migration::backup`        | `migration-*`, `manual-*` snapshots                       | device                               |
| `<data>/logs/`, `<data>/logs/core/`, `<data>/migration.log`                                             | `app_logs_dir`, joins                                      | logger, observers          | logs                                                      | logs                                 |
| `<data>/cache/` (`icons/`, `geodata/`, `pac.js`)                                                        | `cache_dir`, joins                                         | icon service, geodata, PAC | caches                                                    | cache                                |
| `<data>/traffic/`                                                                                       | none (composition join)                                    | traffic storage            | time series                                               | device                               |
| `<data>/scripts/`                                                                                       | `scripts_dir`                                              | script runners             | script runtime dirs                                       | cache                                |
| `<data>/cores/<core>/`                                                                                  | `core_data_dir`                                            | updater                    | core archive extras                                       | binaries                             |
| `<data>/<core binary>`, geodata (`Country.mmdb`, ...)                                                   | `find_binary_path`, none                                   | updater, core working dir  | binaries, geodata, core cache                             | binaries                             |
| `<data>/clash.pid`                                                                                      | `clash_pid_path`                                           | core lifecycle             | PID                                                       | runtime                              |

Eleven paths are built by ad hoc joins outside the resolver (`runtime`, `runtime/control`, `icons`, `cache/icons`, `cache/geodata`, `cache/pac.js`, `logs/core`, `traffic`, `migration-state.yaml`, `migration.log`, the previous typed `clash.yaml`). They are invisible to any enumeration today.

### 1.3 `cfg` in configuration and path code

| Location                                                                        | Usage                                                                    | Effect on sync                                                                        |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| `nyanpasu-config/src/application/mod.rs:257,329`                                | `#[cfg(target_os = "macos")] enable_macos_colored_icons`                 | the schema differs per platform; the field is lost on a round trip through another OS |
| `nyanpasu-config/src/application/mod.rs:47`                                     | `TrayMenuMode::default()` reads `cfg!(windows)`                          | a missing field decodes to a different value per platform                             |
| `nyanpasu-config/src/application/logging.rs:25,30`                              | `LoggingLevel::default()` by `debug_assertions`                          | default differs per build                                                             |
| `nyanpasu-config/src/clash/config/mod.rs:17,19`                                 | `DEFAULT_EXTERNAL_CONTROLLER_PORT` by `debug_assertions`                 | default differs per build                                                             |
| `nyanpasu-config/src/profile/path.rs`                                           | `ManagedProfilePath` and `ExternalProfilePath` validate with `std::path` | no attribute, but the same string is valid on one OS and invalid on another           |
| `nyanpasu-paths/src/lib.rs`, `windows.rs`, `unix.rs`, `registry.rs`, `error.rs` | platform module selection, registry, `.exe` suffix                       | root discovery is OS-specific by nature                                               |
| `nyanpasu-persistence`, `nyanpasu-profile-io`                                   | link detection, exclusive rename, permissions, directory fsync           | filesystem adapters; not schema                                                       |

### 1.4 Portability hazards a naive file sync would hit

- `profiles.yaml` changes on every subscription refresh (`updated_at`, `subscription` usage) and carries `revision`, which the materialization journal compares during recovery. Syncing the file would conflict constantly and could break recovery on the receiving device.
- `LocalBinding::External.target` is an absolute path. `ExternalProfilePath::new("C:\\x.yaml")` fails on Linux, so a catalog written on Windows does not deserialize there at all.
- `ManagedProfilePath` uses `Path::components`: `a\b.yaml` is two components on Windows and one file name on Unix.
- `system_proxy_bypass`, `hotkeys`, `core`, port strategies, TUN and transparent-proxy interfaces describe one machine.
- `ClashGuardOverrides.secret` is the controller secret of one running core.
- Configuration owners hold their document in memory; a file replaced underneath them is overwritten by the next commit. The backup design already records this ("运行中覆盖配置文件会与各 actor 持有的状态冲突").
- Profile source files can carry CRLF when edited on Windows, and file names differing only in case collide on Windows and macOS.

---

## 2. Layout and path manifest

### 2.1 Zones

Every path gets exactly one class. Only `Portable` entries can produce sync items.

```rust
// nyanpasu-paths/src/layout.rs: no cfg, no IO except `enumerate`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathClass {
    /// Syncable configuration or user asset.
    Portable,
    /// Configuration or bookkeeping bound to this device.
    Device,
    /// Durable local state: stores, history, sync baseline, backups.
    State,
    /// Rebuilt from configuration: runtime documents, sockets, PID.
    Derived,
    Cache,
    Logs,
    /// Credentials. Never synced, never backed up, never archived for diagnostics.
    Secret,
    /// Core binaries, geodata and what ships beside a core.
    Binary,
    /// Inputs that only migrations read.
    Legacy,
}
```

### 2.2 Target tree (layout v2)

```text
<config_dir>/
  application.yaml                   Portable (projection)   app/application
  clash-config.yaml                  Portable (projection)   app/clash
  profiles.yaml                      Portable (projection)   app/profiles
  profiles/                          Portable (selected)     profiles/<file>
    .profile-materialization-v1/     Device                  reserved, never enumerated
  icons/                             Portable (binary)       icons/<file>
  local/                             NEW
    sync.yaml                        Device                  sync settings, no secrets
    session-state.yaml               Device                  moved from <config>/session-state.yaml
  migration-state.yaml               Device                  unchanged: it describes this directory
  nyanpasu-config.yaml, clash-guard-overrides.yaml, ...      Legacy

<data_dir>/
  runtime/                           Derived                 moved from <config>/runtime
    clash-config.yaml, candidate*, control/
  sync/                              NEW
    state.redb                       State                   baseline, journal, conflicts
    credentials.json                 Secret                  owner-only permissions
    staging/                         Cache                   verified downloads of one cycle
    snapshots/<cycle>/               State                   pre-images before apply
  storage.db, jobs.redb, backups/, traffic/                  State
  logs/, migration.log                                       Logs
  cache/, scripts/                                           Cache
  cores/, <core binaries>, geodata                           Binary
  clash.pid                                                  Derived
```

Three things move; everything else is registered where it is.

| Change                                             | Reason                                                                                                                                                                                                                                                |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<config>/runtime/` to `<data>/runtime/`           | Generated documents and control sockets are not configuration; the backup copier special-cases them                                                                                                                                                   |
| `<config>/session-state.yaml` to `<config>/local/` | Device state gets a visible home in the config root; `local/` is also where `sync.yaml` lives                                                                                                                                                         |
| Eleven ad hoc joins become resolver methods        | `runtime_dir`, `core_control_dir`, `tray_icons_dir`, `icon_cache_dir`, `geodata_cache_dir`, `pac_script_path`, `core_logs_dir`, `traffic_dir`, `migration_state_path`, `migration_log_path`, `previous_typed_clash_path`, plus the new `sync_*` paths |

The config root therefore holds portable items, `local/`, the migration state and legacy inputs. Names of the three documents do not change: renaming them buys nothing and would touch every migration module.

### 2.3 Manifest and enumeration API

```rust
/// A slash-separated relative key, identical on every platform.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LogicalPath(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind { Application, Clash, Profiles }

/// How an item's portable bytes are produced. The paths crate only names the codec;
/// `nyanpasu-config::sync` implements it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCodec {
    /// Portable projection of a configuration document, canonical JSON.
    Document(DocumentKind),
    /// UTF-8 text, LF line endings, no BOM.
    Text,
    /// Opaque bytes.
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    /// The entry is one file.
    File,
    /// Every regular file under the directory.
    Tree,
    /// Only the files the caller selects, such as files of non-remote profiles.
    SelectedTree,
}

/// One row of the manifest. `'static`: the table is a constant.
pub struct PathEntry {
    pub id: &'static str,
    pub class: PathClass,
    pub root: Root,                     // Config | Data
    pub relative: &'static str,         // forward slashes
    pub sync: Option<SyncRule>,         // Some only when class == Portable
}

pub struct SyncRule {
    pub prefix: &'static str,           // logical prefix: "app", "profiles", "icons"
    pub codec: ItemCodec,
    pub membership: Membership,
}

pub struct SyncItem {
    pub logical: LogicalPath,
    pub physical: Utf8PathBuf,
    pub codec: ItemCodec,
}

pub enum Skipped {
    UnportableName { physical: Utf8PathBuf },
    CaseCollision { logical: LogicalPath, physical: Vec<Utf8PathBuf> },
    Link { physical: Utf8PathBuf },
    NotSelected,
}

impl PathResolver {
    /// Every path the application knows, with its class.
    pub fn manifest(&self) -> &'static [PathEntry];
    /// The physical path of a manifest row.
    pub fn resolve(&self, entry: &PathEntry) -> Utf8PathBuf;
    /// The syncable items present on disk, sorted by logical path.
    pub fn enumerate_sync_items(
        &self,
        selected: &BTreeSet<LogicalPath>,
    ) -> Result<SyncEnumeration, EnumerateError>;
}

pub struct SyncEnumeration {
    pub items: Vec<SyncItem>,
    pub skipped: Vec<Skipped>,
}
```

Rules of `enumerate_sync_items`:

- It walks only `Portable` rows. A directory entry is included only if it is a regular file; links are skipped and reported (`nyanpasu_persistence::is_link` semantics, passed in as a predicate so `layout.rs` stays free of `cfg`).
- Any path component starting with `.` is skipped. That covers `.profile-materialization-v1`, `.atomicwrite*` and editor droppings without a list.
- A name that is not a valid `LogicalPath` segment is skipped as `UnportableName`. Two names equal under ASCII case folding are both skipped as `CaseCollision`. Both are surfaced in the sync status, never silently dropped.
- `SelectedTree` rows include a file only when its logical path is in `selected`. `nyanpasu_config::sync::portable_profile_files(&Profiles)` computes that set: the materialized file of every profile whose source is `Local`. Subscription downloads and orphan files are therefore never enumerated ([§11.2](#112-moving-subscription-downloads-out-of-profiles) covers the alternative).

`LogicalPath` validation is pure string logic and never touches `std::path`:

- one to four segments joined by `/`; total length at most 255 bytes;
- a segment matches `[A-Za-z0-9][A-Za-z0-9._-]{0,127}` and does not end with `.`;
- a segment's stem is not a Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`, any case).

`LogicalPath::to_physical(&self, root: &Utf8Path)` joins segment by segment, so no separator is ever translated. Profile ids are a prefix plus `nanoid(11)` and profile files are `<uid>.<ext>`, so every file the application creates already satisfies the rule. `ManagedProfilePath::new` adopts the same segment check, replacing its `Path::components` logic; a migration check reports existing names that fail it instead of renaming them.

Two tests keep the manifest honest:

- every public path method of `PathResolver` returns a path covered by exactly one manifest row;
- `backup.rs::skipped_paths` is replaced by "rows whose class is not `Portable`, `Device` or `Legacy`", so backup and sync share one classification.

### 2.4 Migration and backward compatibility

The moves are ordinary migration steps, covered by the existing pre-migration backup.

| Step                         | Module (new, document kind where possible) | Action                                                                                              | Rollback             |
| ---------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------- | -------------------- |
| `layout/session_state_local` | `layout`                                   | rename `<config>/session-state.yaml` to `<config>/local/session-state.yaml` if the target is absent | rename back          |
| `layout/runtime_to_data`     | `layout`                                   | remove `<config>/runtime/`; it is regenerated under `<data>/runtime/` on the next build             | none needed: derived |
| `application/tray_menu_auto` | `application` (document)                   | none on disk; the new `auto` variant is additive ([§3.3](#33-removing-cfg-from-the-schemas))        | none                 |

- No syncable document moves or changes its on-disk schema, so a downgrade to the previous release still reads the config root. After a downgrade the old build recreates `<config>/runtime/` and a fresh `session-state.yaml`; the next upgrade's step finds the target present and deletes the stale source after preserving it with `nyanpasu_persistence::preserve`.
- Windows portable layout and the registry `AppDir` keep working unchanged: both only choose roots.
- `runtime_to_data` must land together with the core-manager path change (`actor_v2/local_host.rs`, `client/runtime.rs`). Before merging, confirm that `nyanpasu-service` takes the runtime config path from each request and holds no assumption about its location, and verify on Linux that `<data>/runtime/control/<socket>` stays within the Unix socket path limit for long home directories.
- Sync itself needs no layout migration on the remote: the remote format is versioned separately ([§6.1](#61-remote-layout)).

---

## 3. Platform-neutral portable configuration

### 3.1 Zero-`cfg` zone

The following code contains no `#[cfg(...)]`, `#[cfg_attr(...)]` or `cfg!(...)`, with the single exception of `#[cfg(test)]` on a `mod`:

- `nyanpasu-paths/src/layout.rs`, `nyanpasu-paths/src/logical.rs`;
- `nyanpasu-config/src/application/`, `clash/config/`, `profile/`, `state/`, `sync/`;
- `nyanpasu-contracts/src/sync/`, `nyanpasu-runtime-values/src/sync.rs`, `nyanpasu-core/src/sync/`.

A check in the architecture-ledger script category enforces it by path prefix (`pnpm run lint:architecture-ledger`, with policy tests under `pnpm run test:scripts`). Platform code stays where rust.md §9 wants it: on items in adapter modules (`nyanpasu-paths` root discovery, `nyanpasu-persistence`, `nyanpasu-profile-io`, `nyanpasu-sync-opendal`), outside the zone.

### 3.2 Field scopes

Each document gets a constant table in `nyanpasu-config::sync`, keyed by the serialized top-level name (dotted for a nested key):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldScope { Portable, Device }

pub const APPLICATION_SCOPES: &[(&str, FieldScope)] = &[/* one row per field */];
pub const CLASH_SCOPES: &[(&str, FieldScope)] = &[/* ... */];
```

A test serializes the default value of each document and fails when a key has no row, so a new field cannot be added without deciding its scope.

**`application.yaml`**

| Scope    | Fields                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Portable | `language`, `theme_mode`, `theme_color`, `traffic_graph`, `enable_memory_usage`, `lighten_animation_effects`, `default_latency_test`, `default_latency_timeout_ms`, `enable_builtin_enhanced`, `proxy_layout_column`, `tray_selector_mode`, `enable_tray_traffic`, `stun_servers`, `ip_lookup_providers`, `enable_local_ip_probe`, `update_sources`, `traffic_retention`                                                                                                                                                                                |
| Device   | `app_singleton_port`, `app_log_level`, `core_logs`, `max_log_files`, `max_log_file_size`, `enable_service_mode`, `enable_auto_launch`, `enable_silent_start`, `enable_system_proxy`, `enable_proxy_guard`, `system_proxy_bypass`, `proxy_guard_interval`, `core`, `hotkeys`, `enable_auto_check_update`, `enable_auto_download_update`, `release_channel`, `always_on_top`, `tray_menu_mode`, `window_close`, `network_statistic_widget`, `network_statistic_widget_logo`, `pac_url`, `enable_tray_text`, `use_legacy_ui`, `enable_macos_colored_icons` |

**`clash-config.yaml`**

| Scope    | Fields                                                                                                                                                                                                                                                |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Portable | `overrides.log-level`, `overrides.allow-lan`, `overrides.mode`, `overrides.unified-delay`, `overrides.tcp-concurrent`, `overrides.ipv6`, `web_ui_list`, `enable_clash_fields`, `expand_include_all`, `break_connection`                               |
| Device   | `overrides.secret`, `enable_tun_mode`, `tun`, `tun_stack`, `tun_mips`, `external_controller`, `clash_control_channel`, `clash_ipc_disable_http_controller`, `mixed_port`, `socks_port`, `http_port`, `redir_port`, `tproxy_port`, `transparent_proxy` |

**`profiles.yaml`** uses a typed projection because scope depends on the item:

| Scope    | Content                                                                                                                                                                                                                             |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Portable | `items` in order (`uid`, `name`, `desc`, `custom_name`, definition); `Remote` source keeps `file`, `url`, `option`; `Local` source is emitted as `Managed { file }`; `global_transforms`; `valid`; `folders`; `proxy_group_layouts` |
| Device   | `revision`; `current`; every `updated_at`; `subscription`; `LocalBinding::External { target, mode }`                                                                                                                                |

Notes on the choices that are not obvious:

- `current` is device-bound: a selection made on a laptop must not reload the core on another running machine.
- `core` is device-bound: the selected core may not be installed elsewhere.
- An externally bound profile is portable as content. Other devices receive it as a managed file; the binding device keeps its binding. A remote change to that file is never written through the link on the binding device; it becomes a conflict resolved as `keep_both` ([§7.3](#73-policies)).
- `session-state.yaml` has no portable part.

### 3.3 Removing `cfg` from the schemas

| Today                                                             | Change                                                                                                                                                                                                  |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enable_macos_colored_icons` exists only on macOS                 | Unconditional field on every platform, `#[serde(default)]`, no `specta(skip)`: the generated bindings become identical everywhere, which removes the reason for the skip. Only the macOS tray reads it. |
| `TrayMenuMode::default()` branches on `cfg!(windows)`             | Add variant `Auto` (`"auto"`), make it the default. The tray adapter in the Tauri shell resolves `Auto`, where the platform is known. Stored `native` and `webview` keep their meaning.                 |
| `LoggingLevel::default()` branches on `debug_assertions`          | `Default` is `Info`. A development build raises the level through `FreshInstallDefaults`.                                                                                                               |
| `DEFAULT_EXTERNAL_CONTROLLER_PORT` branches on `debug_assertions` | One constant, `17650`. The development port comes from `FreshInstallDefaults`.                                                                                                                          |

```rust
/// What a shell wants a document to start with when no file exists yet.
/// Supplied by the composition root; schema `Default` impls never read the platform or build.
pub struct FreshInstallDefaults {
    pub app_log_level: LoggingLevel,
    pub external_controller_port: u16,
}
```

The composition root computes it (`cfg!(debug_assertions)` is legitimate there) and passes it to the two state managers, which use it only when the document is absent. This follows the precedent already in the tree: the runtime executor takes the platform as an input (`runtime/executor/mod.rs:101`). All four changed fields are device-scoped, so none of this changes what is synced; it makes the schema one schema.

### 3.4 Canonical portable bytes

Hashes must be equal on every platform and every build for equal content.

- **Document.** The projection is a JSON value: `{"document": "<kind>", "portable_revision": N, "payload": {...}}`, encoded as UTF-8 JSON with object keys sorted and no insignificant whitespace. Ordered collections are arrays in the schema, so sorting keys loses no order. The `_nyanpasu` stamp, YAML formatting and key order of the on-disk file do not enter the hash.
- **Text.** Must decode as UTF-8, else the item fails with `NotUtf8` and is not synced. A leading BOM is removed and CRLF becomes LF, for hashing and on the wire. A file equal to the remote except for line endings is therefore unchanged. Applying a remote change writes LF.
- **Binary.** Bytes as they are.

The hash is SHA-256 (`sha2 0.11` is already used by five workspace crates; items are kilobytes, so BLAKE3 would add a dependency for no measurable gain). An object id is the lowercase hex digest of the canonical bytes.

### 3.5 Version skew and lossless pass-through

`portable_revision` is a per-document integer owned by `nyanpasu-config::sync`, independent of the on-disk `schema_revision`. Additive changes do not bump it; serde defaults and aliases absorb them, as rust.md §6 already requires for persisted types. A breaking change bumps it and adds a pure `upgrade(payload, from) -> payload`.

| Remote revision vs this build  | Behavior                                                                                                                                             |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| equal                          | normal                                                                                                                                               |
| lower                          | upgrade in memory, then merge; the next commit this device writes carries the new revision                                                           |
| higher, or payload undecodable | the item is blocked with `SchemaTooNew`: not applied locally, not overwritten remotely, shown in status as "update this device". Other items proceed |

An older build must not erase what a newer build wrote:

- A top-level key absent from this build's scope table is **pass-through**: the merge copies the remote value and never treats its absence in the local projection as a deletion.
- A profile item is compared after normalizing both sides through this build's typed codec. If the normalized base equals the normalized local item, the item is unchanged and the raw remote value is emitted, including fields this build does not know.

### 3.6 Later items

`storage.db` holds `web:*` keys that users would reasonably expect to follow them (custom CSS, dashboard layout). A redb file cannot be synced as a file. The descriptor model already allows a virtual item (`ui/web-storage`, a per-key map exported through `Storage`), but it is not part of the first delivery and needs its own allowlist of key prefixes.

---

## 4. Crates and ownership

| Crate                         | Adds                                                                                                                    | Layer            |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `nyanpasu-paths`              | `layout`, `logical`: manifest, classes, `LogicalPath`, enumeration                                                      | leaf             |
| `nyanpasu-config`             | `sync`: scope tables, projections, canonical encoding, three-way merge, `SyncSettings`                                  | foundation leaf  |
| `nyanpasu-contracts`          | `sync`: `RemoteStore`, `RemoteStoreFactory`, `SecretStore` ports, `ProviderConfig`, `RemoteCapabilities`, `RemoteError` | foundation leaf  |
| `nyanpasu-runtime-values`     | `sync`: `SyncStatus`, `SyncConflict`, `SyncReport`, `SyncError` wire values                                             | upper foundation |
| `nyanpasu-core`               | `sync/`: commit format, pure planner, `SyncActor`, `SyncClient`, redb state store, apply ports; facade methods          | core             |
| `nyanpasu-sync-opendal` (new) | `OpendalRemoteStore`, `OpendalStoreFactory`; the only crate that names `opendal`                                        | implementation   |
| `nyanpasu-rpc`                | `commands/sync.rs`, `SyncStatusChanged` event                                                                           | RPC adapter      |
| `backend/tauri`               | composition: build the factory and secret store, inject, register the job                                               | shell            |

This matches [Backend packages](../development/backend-packages.md): the implementation crate depends on the foundation layer only, core consumes ports and never links OpenDAL in production. Adding the crate updates the dependency policy tests, `STATIC_GATE_PREFIXES` and the musl package lists in the same change; run `pnpm run lint:backend-boundaries`.

`ClientSetupArgs` gains `sync: Option<SyncAdapters>`. A shell that passes `None` keeps every sync command registered and answering `unsupported`, as the RPC guide requires for a missing capability. `nyanpasu-server` passes `None` in the first delivery.

---

## 5. OpenDAL integration layer

### 5.1 Port

The port is byte-level and knows nothing about commits, so a second implementation or a test fake is small.

```rust
// nyanpasu-contracts/src/sync/ports.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCapabilities {
    /// Create-if-absent is advertised by the backend. The vault self-test decides whether it is trusted.
    pub create_if_absent: bool,
    /// Listing returns a modification time for objects.
    pub list_last_modified: bool,
    /// An empty object can be written.
    pub write_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub key: String,                       // relative to the vault root, forward slashes
    pub size: u64,
    pub last_modified: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutCondition { Any, IfAbsent }

#[async_trait::async_trait]
pub trait RemoteStore: Send + Sync + 'static {
    fn capabilities(&self) -> RemoteCapabilities;
    /// Reachability and credentials; fails with a classified error.
    async fn check(&self) -> Result<(), RemoteError>;
    /// Every object under `prefix`, recursively.
    async fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, RemoteError>;
    async fn get(&self, key: &str) -> Result<Bytes, RemoteError>;
    async fn put(&self, key: &str, body: Bytes, condition: PutCondition) -> Result<(), RemoteError>;
    async fn delete(&self, key: &str) -> Result<(), RemoteError>;
}

#[derive(Debug, Snafu)]
pub enum RemoteError {
    NotFound { key: String },
    /// `PutCondition::IfAbsent` found an object.
    AlreadyExists { key: String },
    Unauthorized { source: BoxedCause },
    QuotaExceeded { source: BoxedCause },
    /// Network, timeout, 5xx, rate limit. Retried inside the adapter first.
    Unavailable { source: BoxedCause },
    /// The condition or operation is not supported by this backend.
    Unsupported { operation: &'static str },
    Other { source: BoxedCause },
}

pub trait RemoteStoreFactory: Send + Sync + 'static {
    /// The provider kinds compiled into this build.
    fn kinds(&self) -> Vec<ProviderKind>;
    fn open(&self, config: &ProviderConfig, secrets: &ProviderSecrets)
        -> Result<Arc<dyn RemoteStore>, RemoteError>;
}
```

The mapping from `opendal::ErrorKind` is fixed in one function: `NotFound`; `ConditionNotMatch` and `AlreadyExists` to `AlreadyExists`; `PermissionDenied` to `Unauthorized`, `RateLimited` and temporary errors to `Unavailable`, `Unsupported` to `Unsupported`. `stat`, etags, rename and copy are deliberately absent from the port: the protocol in [§6](#6-incremental-sync-protocol) does not need them, which is what makes it uniform across providers.

### 5.2 Provider configuration

```rust
// nyanpasu-contracts/src/sync/provider.rs: serialized into local/sync.yaml, no secrets
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    Webdav { endpoint: Url, root: String, username: Option<String> },
    S3 { endpoint: Option<Url>, region: String, bucket: String, root: String, access_key_id: String,
         virtual_host_style: bool },
    Fs { root: String },
    /// Any other compiled-in OpenDAL scheme, configured by its documented keys.
    /// Keys that name secrets are rejected here and must be supplied as `ProviderSecrets`.
    Custom { scheme: String, options: BTreeMap<String, String> },
}

/// Never serialized with the settings; held by the `SecretStore`.
pub struct ProviderSecrets(BTreeMap<String, SecretString>);
```

Typed variants give the settings UI real forms for the common providers. `Custom` is the uniform path: its `options` are the provider's OpenDAL configuration keys, so enabling a further service is a feature flag, one arm in the factory and no protocol change. Dropbox, Google Drive and OneDrive get typed variants when their OAuth flow lands ([§5.6](#56-credentials)).

### 5.3 Building an operator

Verified against 0.59.5 ([Appendix A](#appendix-a-opendal-0595-probe)):

- `Operator::via_iter(scheme, ...)` resolves the scheme through a process-wide `OperatorRegistry`, which the facade crate fills from a `ctor` constructor when its default feature `auto-register-services` is on. With default features off, `via_iter("webdav", ...)` fails with "scheme is not registered".
- `Operator::from_iter::<services::Webdav>(pairs)` selects the builder statically and needs no registry.
- The default HTTP transport is also process-wide and first-installed-wins. `Operator::with_context(OperationContext::new().with_http_transport(...))` sets a transport on one operator. `opendal-http-transport-reqwest 0.59.5` wraps a `reqwest 0.13` client, the major version this workspace already pins.

The adapter therefore uses neither global:

```rust
// nyanpasu-sync-opendal/src/factory.rs
fn build(config: &ProviderConfig, secrets: &ProviderSecrets, http: reqwest::Client)
    -> Result<Operator, opendal::Error>
{
    let pairs = options(config, secrets);          // BTreeMap<String, String> -> iterator
    let operator = match config {
        ProviderConfig::Webdav { .. } => Operator::from_iter::<services::Webdav>(pairs)?,
        ProviderConfig::S3 { .. } => Operator::from_iter::<services::S3>(pairs)?,
        ProviderConfig::Fs { .. } => Operator::from_iter::<services::Fs>(pairs)?,
        ProviderConfig::Custom { scheme, .. } => custom(scheme, pairs)?,   // closed match over compiled-in schemes
    };
    Ok(operator
        .with_context(OperationContext::new()
            .with_http_transport(HttpTransporter::new(ReqwestTransport::new(http))))
        .layer(TimeoutLayer::new())
        .layer(RetryLayer::new()))
}
```

- The `reqwest::Client` is built by the shell from the existing `nyanpasu-net` client policy, so sync follows the same proxy choice as subscriptions (`direct`, `system` or through the running core, a `SyncSettings` field). No second TLS stack or client policy appears.
- Deadlines and retries live here, in the network adapter, as the architecture guide requires; `SyncActor` awaits the real result without a timeout of its own.
- `RemoteStore::list` calls `list_with(prefix).recursive(true)`. OpenDAL emulates recursion where the service lacks it (WebDAV, `fs`, OneDrive), so the port stays uniform.
- The vault root is the provider `root` joined with a fixed `nyanpasu-sync/` segment, so a user pointing at an existing folder cannot make sync list or garbage-collect foreign objects.

### 5.4 Features and build footprint

`backend/Cargo.toml` gains one workspace dependency with every default off; only `nyanpasu-sync-opendal` uses it:

```toml
[workspace.dependencies]
opendal = { version = '0.59.5', default-features = false, features = [
  'executors-tokio',
  'layers-retry',
  'layers-timeout',
] }
opendal-http-transport-reqwest = { version = '0.59.5', default-features = false }
```

```toml
# nyanpasu-sync-opendal/Cargo.toml
[features]
default = ['webdav', 's3', 'fs']
webdav = ['opendal/services-webdav']
s3 = ['opendal/services-s3']
fs = ['opendal/services-fs']
dropbox = ['opendal/services-dropbox']
gdrive = ['opendal/services-gdrive']
onedrive = ['opendal/services-onedrive']
```

- Left off deliberately: `auto-register-services` (`ctor`, life before `main`), `http-transport-reqwest*` on the facade (it would install a process-wide default), `layers-logging` and `layers-concurrent-limit`. The memory service is always compiled into `opendal-core` and serves tests.
- Shells opt in to providers explicitly (`default-features = false` plus a list), the way they already select script engines.
- Measured with the workspace release profile (`opt-level = 's'`, LTO, one codegen unit, stripped) on x86_64 Linux, against a baseline binary that already links `reqwest 0.13` with rustls and `tokio`:

| Build                                   | Size    | Delta    |
| --------------------------------------- | ------- | -------- |
| baseline (`reqwest` + `tokio` + memory) | 3.83 MB |          |
| + `fs`, `webdav`, `s3`                  | 6.15 MB | +2.32 MB |
| + `dropbox`, `gdrive`, `onedrive`       | 6.76 MB | +0.61 MB |

- Against `backend/Cargo.lock`, the default set adds 21 packages: 8 OpenDAL crates and 13 others (`reqsign-*`, `crc-fast`, `rust-ini`, `ordered-multimap`, `dlv-list`, `const-random*`, `tiny-keccak`, `asyncband`, `utf_types`). Most arrive with S3 request signing. `reqwest`, `rustls`, `hyper` and `tokio` are shared with the existing tree.
- The numbers are from a probe binary, not from the application; re-measure on the Tauri release bundle and the musl server before enabling a feature set there.

### 5.5 Capability heterogeneity

`Operator::info().capability()` as reported by 0.59.5 for the planned providers:

| Capability                     | `fs`          | `memory` | `webdav`         | `s3` | `dropbox` | `gdrive` | `onedrive`    |
| ------------------------------ | ------------- | -------- | ---------------- | ---- | --------- | -------- | ------------- |
| `write_with_if_not_exists`     | yes           | yes      | no               | yes  | no        | no       | no            |
| `write_with_if_match`          | no            | no       | no               | yes  | no        | no       | yes           |
| `list_with_recursive` (native) | no (emulated) | yes      | no (emulated)    | yes  | yes       | yes      | no (emulated) |
| `rename`                       | yes           | no       | yes              | no   | yes       | yes      | yes           |
| `write_can_empty`              | yes           | yes      | yes              | yes  | no        | no       | no            |
| etag on `stat`                 | none          | none     | server-dependent | yes  | n/a       | n/a      | yes           |
| `last_modified` on list        | yes           | none     | server-dependent | yes  | yes       | yes      | yes           |

Consequences, each of which the protocol absorbs instead of branching per provider:

| Difference                                            | How the design handles it                                                                                                                      |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| No conditional write on WebDAV, Dropbox, Google Drive | Commit naming mode `content`: forks are possible, detected and merged ([§6.6](#66-concurrency-across-devices))                                 |
| Advertised conditional create that a server ignores   | The vault self-test writes a probe twice with `IfAbsent`; a second success means the mode is `content` regardless of the capability flag       |
| No atomic overwrite, torn uploads                     | Nothing is overwritten. A commit name contains its hash or is created exclusively; a manifest whose content does not hash to its id is ignored |
| Eventual consistency of listings                      | A stale listing only delays visibility. A device that misses a sibling commit converges through the divergence path on a later cycle           |
| No etag, no `stat` conditions                         | Never used. Change detection is by commit name from one listing                                                                                |
| Empty objects not writable                            | No marker files. Every object the protocol writes has content                                                                                  |
| No `last_modified`                                    | Object garbage collection is skipped on that backend; commits carry their own timestamps                                                       |
| No rename, no copy                                    | Never used                                                                                                                                     |

### 5.6 Credentials

```rust
// nyanpasu-contracts/src/sync/ports.rs
pub trait SecretStore: Send + Sync + 'static {
    fn load(&self) -> Result<Option<ProviderSecrets>, SecretStoreError>;
    fn save(&self, secrets: &ProviderSecrets) -> Result<(), SecretStoreError>;
    fn clear(&self) -> Result<(), SecretStoreError>;
}
```

- The default implementation is a file, `<data>/sync/credentials.json`, created with owner-only permissions the way `ProfileFileService::write_private_file_new` creates private files, with the permission code in the adapter crate. It lives in the data root, is classed `Secret`, and that class is excluded from sync enumeration, from `create_backup`, and from the diagnostics archive.
- An OS keychain adapter can replace it behind the same port later; it is not in the first delivery because the Linux secret service is absent on headless hosts and the file must exist as a fallback anyway.
- Secrets are write-only over RPC: `configure_sync` accepts them, no query returns them, and `get_sync_settings` reports only whether they are present. Provider errors are logged through the existing redaction helpers; `ProviderSecrets` has no `Debug` that prints values.
- `ProviderConfig` rejects known secret keys in `Custom.options` (`password`, `secret_access_key`, `access_token`, `refresh_token`, `client_secret`, `token`), so a secret cannot be typed into the non-secret settings by mistake.
- Sync settings and credentials are device-scoped, so a synced vault never contains the means to reach itself or another vault.
- OAuth providers need an application registration and a browser redirect handled by the shell. OpenDAL accepts `refresh_token`, `client_id` and `client_secret` for them; the tokens would go through `SecretStore`. That flow is a later phase.

---

## 6. Incremental sync protocol

### 6.1 Remote layout

```text
<provider root>/nyanpasu-sync/
  format.json                              written once at vault creation
  commits/<generation:012>-<commit id>.json     naming mode `content`
  commits/<generation:012>.json                 naming mode `exclusive`
  objects/<aa>/<sha256>                    canonical portable bytes, content-addressed
  probe/<nonce>                            capability self-test, deleted afterwards
```

```json
{
  "format": 1,
  "vault_id": "v_4f0c...",
  "created_at": "2026-11-14T12:00:00Z",
  "commit_naming": "content",
  "encryption": { "kind": "none" }
}
```

- `format` is the remote protocol version. A device that reads a higher number stops with `VaultTooNew` and writes nothing.
- `vault_id` is random. The baseline records it, so a root that was wiped and recreated is recognised as a different vault instead of as "everything was deleted".
- `commit_naming` is decided once, by the creating device's self-test, and binds every device. A device whose backend cannot honour `exclusive` refuses to write to such a vault.

### 6.2 Commit manifest

A commit is the complete portable state at one generation. Items are few and small, so a full table costs a few kilobytes and removes any need to replay history.

```json
{
  "format": 1,
  "generation": 42,
  "id": "9b1d...",
  "parent": "5e77...",
  "ancestors": ["5e77...", "c0a1...", "..."],
  "device": { "id": "d_Qm3x...", "name": "keiko-laptop" },
  "created_at": "2026-11-14T12:03:11Z",
  "app_version": "2.0.0",
  "items": {
    "app/application": {
      "codec": "document",
      "sha256": "ab12...",
      "size": 812,
      "portable_revision": 1,
      "changed": { "generation": 40, "device": "d_Qm3x..." }
    },
    "profiles/pK3mA9xZq2Lw.yaml": {
      "codec": "text",
      "sha256": "77fe...",
      "size": 5120,
      "changed": { "generation": 42, "device": "d_Qm3x..." }
    },
    "profiles/sOld0000000a.js": {
      "deleted": {
        "generation": 41,
        "device": "d_8hTz...",
        "at": "2026-11-10T08:00:00Z",
        "last_sha256": "0c9e..."
      }
    }
  }
}
```

- `id` is the first 32 hex characters of the SHA-256 of the canonical JSON with the `id` field removed. A reader recomputes it; a mismatch means a torn or foreign object, which is ignored and later collected.
- `ancestors` lists up to 64 preceding commit ids, newest first. It answers "is my baseline an ancestor of the head" and finds a common ancestor without fetching history.
- A **tombstone** records a deletion. Three-way comparison against a baseline does not need it; a device without a baseline does, to tell "deleted elsewhere" from "new here", and the conflict view uses it to say who deleted what. Tombstones are dropped after 180 days.
- **Renames** are not modelled. Document keys are fixed and profile files are named by profile id, so renaming a profile changes catalog metadata only. A file that does move is a deletion plus an addition whose object already exists remotely, so no content is transferred.

### 6.3 Local sync state

`<data>/sync/state.redb`, opened by the composition root and owned by `SyncActor` through a `SyncStateStore` port:

| Table            | Content                                                                                                                           |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `meta`           | `device_id` (random, created on first enable), `vault_id`, baseline `{generation, id, ancestors}`, `pending_commit`, last GC time |
| `baseline_items` | logical path to the baseline item entry (hash, size, codec, revision or tombstone)                                                |
| `baseline_docs`  | canonical bytes of each baseline document, needed as the base of a field merge                                                    |
| `journal`        | the current cycle: id, phase, planned actions, uploaded object ids, applied items                                                 |
| `conflicts`      | open conflicts and recorded resolutions                                                                                           |

- The baseline is the last state this device and the remote agreed on. It advances in one redb write transaction at the end of a cycle, so it is never half-updated.
- There is no stat cache. A cycle projects three documents and hashes a few dozen small files; subscription downloads are not items. A cache would add an invalidation problem to save milliseconds.
- If the file is missing or unusable it is preserved with `nyanpasu_persistence::preserve` and the device rejoins the vault as a device without a baseline ([§6.8](#68-first-sync-and-rejoin)).

### 6.4 Change detection

Per item, with `B` the baseline hash, `L` the local hash and `R` the remote head hash (absent counts as a value):

| `L` vs `B` | `R` vs `B`        | Result                                            |
| ---------- | ----------------- | ------------------------------------------------- |
| same       | same              | nothing                                           |
| changed    | same              | push                                              |
| same       | changed           | pull                                              |
| changed    | changed, `L == R` | converged: baseline moves, nothing is transferred |
| changed    | changed, `L != R` | merge or conflict ([§7](#7-conflict-protocol))    |

Only the delta moves: a push uploads objects the head manifest does not reference; a pull downloads objects whose hash differs from local. The local side is obtained without racing the owners: documents come from the owners' committed snapshots together with their version, files through `ProfileFsPort`.

### 6.5 Cycle state machine

```text
Idle
 └─ Scan         project documents, enumerate and hash files          (local only)
 └─ Fetch        list commits/; get the head manifest if its name is not the baseline's
 └─ Plan         pure: (baseline, local, remote, policy, resolutions) -> actions + conflicts
 └─ Download     objects for pulls and merges into staging/, verify SHA-256
 └─ Snapshot     copy pre-images of every item about to be changed locally
 └─ Apply        through domain owners, each with its planned version or hash
 └─ Upload       objects for pushes and merge results
 └─ Commit       put the manifest at generation head + 1
 └─ Confirm      list commits/ once more; a different head schedules an immediate cycle
 └─ Finalize     advance the baseline in one transaction; clear the journal
Idle
```

`Plan` is a pure function in `nyanpasu-core::sync::plan` over plain values, which is where most of the test effort goes.

**Round trips.** `r` is one request to the provider.

| Situation                 | Requests                                               |
| ------------------------- | ------------------------------------------------------ |
| Nothing changed anywhere  | 1 (`list commits/`; the head name equals the baseline) |
| Pull of `k` changed items | 2 + `k`                                                |
| Push of `k` changed items | 1 + `k` + 1 commit + 1 confirm                         |
| Both, with merges         | 2 + downloads + uploads + 2                            |

Object existence is never probed: the head manifest says which objects the remote has. Downloads and uploads run with a small fixed concurrency inside one cycle.

**Crash and partial-sync recovery.** Every phase is safe to abandon because planning is by content, not by remembered intent.

| Interrupted during | State left behind                        | Next cycle                                                                                                   |
| ------------------ | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Scan, Fetch, Plan  | nothing                                  | starts over                                                                                                  |
| Download           | files in `staging/`                      | staging is cleared; verified objects are re-fetched                                                          |
| Apply              | some items applied, baseline old         | applied items now equal the remote, so they plan as "converged"; merged items re-merge to the same result    |
| Upload             | unreferenced objects                     | the journal lists them so they are not re-sent; otherwise they are collected later                           |
| Commit             | `pending_commit` recorded before the put | the head is the pending commit: the baseline adopts it without transfer. If the put never landed, plan again |
| Finalize           | commit visible, baseline old             | same as above                                                                                                |

- A merge is deterministic for a fixed policy and resolution set, so re-running it after a crash reproduces the same bytes.
- Unresolved conflicts never block the cycle: a conflicted item keeps its remote value in the new commit and its old entry in the baseline, and every other item proceeds.
- A failed item (unreadable file, `NotUtf8`, owner rejected the value) is reported and skipped; the cycle result is `Degraded`, not failed, when anything else succeeded.

### 6.6 Concurrency across devices

**Within one device.** `SyncActor`'s mailbox runs one cycle at a time; the single-instance claim and redb's file lock keep a second process out. A local edit that lands during a cycle is caught at apply time: each apply carries the version (documents) or hash (files) the plan saw, and a mismatch defers that item to the next cycle.

**Between devices, naming mode `exclusive`.** The commit for generation `n` is written to `commits/<n>.json` with `PutCondition::IfAbsent`. The create is atomic, so exactly one device wins generation `n`. A loser gets `AlreadyExists`, discards nothing local, and starts a new cycle, which pulls generation `n` first. History is linear. There is no lock object and therefore no stale-lock problem.

**Between devices, naming mode `content`.** The commit is written to `commits/<n>-<id>.json`. Two devices can both write generation `n`. The head of a listing is the commit with the highest generation, lowest id on a tie. A device compares the head with its baseline:

| Relation                                                            | Meaning      | Action                                                                                                                                                                                                           |
| ------------------------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| head is the baseline                                                | no change    | nothing                                                                                                                                                                                                          |
| baseline id is in `head.ancestors`                                  | fast-forward | normal three-way plan with the baseline as base                                                                                                                                                                  |
| otherwise, and a common id exists in both `ancestors` lists         | diverged     | fetch the common ancestor's manifest and base documents; three-way merge with base = common ancestor, local = this device's current state, remote = head; commit with `parent = head` and `merged = [orphan id]` |
| no common ancestor within 64 generations, or a different `vault_id` | unrelated    | treat as a device without a baseline ([§6.8](#68-first-sync-and-rejoin))                                                                                                                                         |

- The base in the diverged case must be the common ancestor, not the device's own baseline. Using the baseline would read the other branch's lack of this device's changes as a remote revert and silently undo them.
- A device whose commit lost the tie loses nothing: its changes are still its local state and re-enter through the merge.
- The same rule covers a backend that silently ignores `IfAbsent` after passing the self-test: an overwritten commit shows up as a baseline id that is not an ancestor of the head.
- The `Confirm` listing narrows the window in which a fork goes unnoticed to one request; it is an optimisation, not a correctness step.

### 6.7 Garbage collection

At most once a day, by whichever device syncs:

- keep the newest 30 commits and every commit younger than 30 days; delete older ones;
- delete an object when no kept commit references it and its `last_modified` is older than 7 days, which protects an upload whose commit has not landed yet;
- on a backend without `last_modified` in listings, skip object collection;
- delete `probe/` leftovers and manifests whose id does not verify, when older than a day.

A device that stays offline past the retention window finds no common ancestor and takes the unrelated path, which never deletes local data.

### 6.8 First sync and rejoin

A device without a baseline cannot tell an addition from a deletion, so nothing is inferred:

| Local   | Remote                                              | Action                                                                                                                        |
| ------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| present | absent, no tombstone                                | push                                                                                                                          |
| present | tombstone whose `last_sha256` equals the local hash | delete locally (after snapshot): it is the item that was deleted                                                              |
| present | tombstone, different hash                           | push: it was edited here since                                                                                                |
| absent  | present                                             | pull                                                                                                                          |
| present | present, equal                                      | converged                                                                                                                     |
| present | present, different                                  | documents: fields that differ follow the join strategy; catalog: union by profile id; files: conflict under the active policy |

`enable_sync` takes an explicit `JoinStrategy` (`PreferRemote`, `PreferLocal`, `Review`) used only for this first cycle, and the client takes a full `create_backup` (new `BackupKind::Sync`, prefix `sync-`, keep 3) before it. An empty remote is initialised instead: self-test, `format.json`, then a normal push.

### 6.9 Reserved: client-side encryption

Profiles contain subscription URLs with tokens and proxy credentials, and the remote is storage the user chose. Encryption is not in the first delivery, but the format leaves room: `format.json.encryption` selects a scheme, objects and manifests would be sealed with a key derived from a passphrase held in `SecretStore`, and object names would become a keyed hash so equal content does not reveal itself. Until then the settings UI states plainly that the vault is stored unencrypted.

---

## 7. Conflict protocol

### 7.1 Detection

A conflict exists only when both sides changed an item relative to the base and the results differ after canonicalization. Detection is per unit, and the unit depends on the codec:

| Item                                        | Unit                                    | Both changed, different units | Both changed, same unit                                              |
| ------------------------------------------- | --------------------------------------- | ----------------------------- | -------------------------------------------------------------------- |
| `app/application`, `app/clash`              | portable field                          | merged automatically          | field conflict                                                       |
| `app/profiles` items                        | profile item by `uid`                   | merged automatically          | item conflict                                                        |
| `app/profiles` item order                   | the id sequence                         | n/a                           | never a conflict: remote order, then local-only items in local order |
| `app/profiles` folders, layouts             | folder by `FolderId`, layout by profile | merged automatically          | conflict, resolved as a field conflict                               |
| `app/profiles` `global_transforms`, `valid` | whole list                              | n/a                           | field conflict                                                       |
| `profiles/<file>`, `icons/<file>`           | whole file                              | n/a                           | file conflict                                                        |

Kinds:

```rust
// nyanpasu-runtime-values/src/sync.rs
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides changed the same unit to different values.
    ModifyModify,
    /// One side deleted what the other modified.
    ModifyDelete { deleted_by: ConflictSide },
    /// The remote deletes the profile this device has active.
    DeleteActive,
    /// The merged value fails domain validation on this device.
    InvalidMerge { reason: String },
    /// The remote value needs a newer application.
    SchemaTooNew { remote_revision: u32, supported: u32 },
}
```

- Line-level merging of YAML or scripts is not attempted. Two edits of one proxy list can merge textually into a configuration that parses and routes wrongly; a file is replaced or kept, never spliced.
- `DeleteActive` is a conflict under every policy, including `remote_wins`: sync never removes the configuration a running core is using.
- After a catalog merge the result goes through the existing validation and `repair_folders`. A reference to a profile the other side deleted (in `transforms`, `global_transforms`, a composition, a folder) is dropped and listed in the report. A merged document that the owner rejects (for example a cross-field rule involving device fields) becomes `InvalidMerge` and is not applied.

### 7.2 Three-way merge

```rust
// nyanpasu-config/src/sync/merge.rs: pure
pub struct MergeInput<'a> {
    pub base: Option<&'a PortableDocument>,
    pub local: &'a PortableDocument,
    pub remote: &'a PortableDocument,
    pub policy: ConflictPolicy,
    pub resolutions: &'a [Resolution],
}

pub struct MergeOutput {
    /// Units without an open conflict already merged; conflicted units hold the remote value.
    pub merged: PortableDocument,
    pub conflicts: Vec<UnitConflict>,
    pub dropped_references: Vec<DroppedReference>,
}

pub fn merge_document(input: MergeInput<'_>) -> Result<MergeOutput, MergeError>;
```

Applying a merged projection is the inverse of projecting: the owner's current document supplies every `Device` field and the merged projection supplies every `Portable` field. For profiles, items the remote added get device state initialised empty (no `updated_at`, no subscription info), and a new `Remote` item triggers the existing refresh job to download its file.

### 7.3 Policies

`SyncSettings.conflict_policy`, default `manual`. The policy applies only to units in conflict; non-overlapping changes always merge.

| Policy        | Field in a document                                        | Profile item or file                                                                                                                                                     |
| ------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `manual`      | stays local here, remote stays remote, conflict stays open | same; the conflict stays open until resolved                                                                                                                             |
| `local_wins`  | local value is committed                                   | local version is committed                                                                                                                                               |
| `remote_wins` | remote value is applied                                    | remote version is applied                                                                                                                                                |
| `keep_both`   | as `remote_wins`; the local value is kept in the snapshot  | remote keeps the original identity; the local version becomes a new profile `"<name> (conflict, <device>, <date>)"` with a fresh id, committed so every device sees both |

- `keep_both` is deterministic and convergent: the committed remote value always keeps the original id, so two devices that resolve the same conflict produce two distinct conflict copies and the same original.
- A `ModifyDelete` under `keep_both` keeps the modified version. Under `local_wins` and `remote_wins` the named side decides, including a deletion.
- For `icons/` there is no catalog to hold a copy; `keep_both` behaves as `remote_wins` and the local file survives in the snapshot.
- A manual resolution is a recorded value (`Resolution { conflict_id, choice }` with `choice` one of `Local`, `Remote`, `KeepBoth`) that the next plan consumes as input. Resolving is therefore replayable and crash-safe like the rest of the cycle.

### 7.4 Safety snapshots

- Before the first apply of a cycle, `Snapshot` copies the pre-image of every item the cycle will change or delete into `<data>/sync/snapshots/<cycle id>/`: the full on-disk document for a document item (so device fields are restorable too), the file for a file item, plus a `manifest.json` naming the cycle, the remote commit and each item's hashes before and after.
- The snapshot is written to a `.partial` directory and renamed, as `backup.rs` does. If it cannot be written, nothing is applied.
- Retention: the newest 20 snapshots and anything younger than 30 days.
- `restore_sync_snapshot` re-applies the pre-images through the owners. To sync, the restored state is an ordinary local change, which the next cycle pushes.
- The first sync on a device additionally takes a full `create_backup` ([§6.8](#68-first-sync-and-rejoin)).

---

## 8. Application integration

### 8.1 Owner and ports

```text
NyanpasuClient
  └─ SyncClient ── SyncActor (owns settings, baseline, journal, conflicts, current cycle)
        ├─ Arc<dyn RemoteStoreFactory>      nyanpasu-sync-opendal, injected by the shell
        ├─ Arc<dyn SecretStore>             file adapter, injected by the shell
        ├─ Arc<dyn SyncStateStore>          redb adapter in core
        ├─ PortableStatePort                read snapshots + versions; apply with expected version
        ├─ PathResolver, Arc<dyn ProfileFsPort>
        └─ Arc<dyn SyncStatusSink>          status fan-out to the UI
```

- `SyncActor` is an actor because it owns long-lived mutable state and a long-running operation. Planning, projection, merge and canonicalization are pure services.
- `PortableStatePort` is implemented in core over the existing typed clients. Its apply operations build on the existing `Replace` messages of `ApplicationActor`, `ClashConfigActor` and the profiles actor. Two additions are needed. `Replace` gains an expected version: the owners already commit through `ReplaceIfVersionResult` against their own snapshot, and the caller's planned version is passed down to that comparison so a stale plan is refused with the existing `VersionConflict` error. Profiles also need a single message that commits a catalog replacement together with a set of file writes and deletions, so a pulled profile and its file land in one materialization transaction instead of two commits.
- Dependencies run one way: `SyncActor` calls the configuration owners; they never call it. Local commits reach sync only as a "dirty" notification through the existing `CommitNotifications` fan-out, which debounces an automatic cycle.
- Applied changes are ordinary mutations, so runtime rebuild, effects and `StateChangedEvent` follow without sync-specific code. A post-commit effect failure is reported as a degradation on the cycle, not as a failed sync.
- A caller that drops does not cancel a cycle. Shutdown cancels the root token; the actor refuses new cycles and finishes or abandons the current phase, which [§6.5](#65-cycle-state-machine) makes safe.

### 8.2 Scheduling

Sync cycles are jobs in the existing jobs subsystem: one job key `sync.config`, manual trigger plus an interval derived from `SyncSettings.interval_minutes` (zero means manual only, as for remote profiles). The handler captures only `SyncClient`. Run history and logs then appear in the existing jobs UI, and the "no overlap" rule gives `Skipped(Busy)` for a scheduled cycle that meets a manual one.

Triggers: application start (after startup settles), interval, debounced local change, and `sync_now`.

### 8.3 Settings

`<config>/local/sync.yaml`, owned by `SyncActor`, stamped like other documents:

```rust
// nyanpasu-config/src/sync/settings.rs
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncSettings {
    pub enabled: bool,
    pub provider: Option<ProviderConfig>,
    pub device_name: String,
    pub conflict_policy: ConflictPolicy,
    pub interval_minutes: u64,
    pub sync_on_change: bool,
    pub network: SyncNetwork,              // Direct | System | ThroughCore
}
```

It is not part of `application.yaml` for two reasons: `patch_app_config` is exposed over HTTP, and the RPC guide asks for a dedicated command when a setting's privilege class differs. Pointing sync at a storage endpoint moves subscription tokens there.

### 8.4 Facade and RPC

Facade methods on `NyanpasuClient` (domain operations, no generic access to the actor):

| Method                              | Kind     | Purpose                                                      |
| ----------------------------------- | -------- | ------------------------------------------------------------ |
| `sync_status()`                     | query    | current `SyncStatus`                                         |
| `sync_settings()`                   | query    | settings with `credentials_present: bool`, never the secrets |
| `sync_provider_kinds()`             | query    | provider kinds compiled into this build                      |
| `list_sync_conflicts()`             | query    | open conflicts with both values, or a diff summary for files |
| `list_sync_snapshots()`             | query    | pre-image snapshots                                          |
| `test_sync_connection(input)`       | mutation | `check` plus the capability self-test, without saving        |
| `configure_sync(input)`             | mutation | save provider, secrets and options                           |
| `enable_sync(join)`                 | mutation | first cycle with a `JoinStrategy`                            |
| `disable_sync()`                    | mutation | stop syncing; local data and the remote vault stay           |
| `sync_now()`                        | mutation | run a cycle and return its `SyncReport`                      |
| `resolve_sync_conflict(id, choice)` | mutation | record a resolution and run a cycle                          |
| `restore_sync_snapshot(id)`         | mutation | re-apply pre-images                                          |

RPC commands are thin wrappers in `backend/nyanpasu-rpc/src/commands/sync.rs`, declared with `#[nyanpasu_macro::rpc(...)]` and registered in `backend/tauri/src/specta_export.rs` as listed. None of them declares `rpc(http)` in the first delivery: a browser caller must not be able to redirect a device's configuration to another endpoint or read conflict contents. Exposure to WebUI or the local socket is a separate reviewed opt-in. Regenerate the command catalog and the frontend bindings; do not hand-edit them.

### 8.5 Status and events

```rust
// nyanpasu-runtime-values/src/sync.rs
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncStatus {
    /// Increases with every change, so a delayed read cannot replace a newer value.
    pub revision: u64,
    pub availability: SyncAvailability,        // Unsupported | Disabled | Enabled
    pub phase: SyncPhase,                      // Idle | Scanning | Fetching | ... | Finalizing
    pub progress: Option<SyncProgress>,        // items done / total, bytes
    pub last_success_at: Option<i64>,
    pub last_result: Option<SyncOutcome>,      // Synced | UpToDate | Degraded | Failed
    pub last_error: Option<SyncError>,
    pub open_conflicts: u32,
    pub blocked_items: Vec<BlockedItem>,       // SchemaTooNew, NotUtf8, UnportableName, CaseCollision
    pub head: Option<SyncHead>,                // generation, device name, time
}
```

- `SyncError` is a typed, serializable enum (`Unauthorized`, `Unreachable`, `QuotaExceeded`, `VaultTooNew`, `VaultMismatch`, `ProviderLacksConditionalCreate`, `Storage`, ...), so the UI localizes by variant instead of parsing text.
- `SyncStatusChanged` is a full-value status event carrying `SyncStatus`, published through `nyanpasu-rpc::events` like the application-update snapshot. The frontend takes the initial value from `get_sync_status`, applies events, refetches on transport resync and drops any value with a lower `revision`.
- Phase and progress changes are coalesced before publication so a cycle emits a handful of events, not one per object.

### 8.6 Frontend

A "Sync" section under settings, using `@nyanpasu/rpc` and `@nyanpasu/query` only:

- provider form generated from `sync_provider_kinds`, a "test connection" action, write-only credential fields;
- status card: phase, last sync, head device, a "sync now" action that is disabled while a cycle runs;
- a conflict list with per-conflict choices (local, remote, keep both) and a side-by-side view for documents;
- blocked items with the reason and what to do ("update this device", "rename the file");
- snapshot list with restore;
- a persistent indicator when conflicts are open, in the place the update indicator uses.

Text goes through the existing i18n workflow for all shipped locales.

---

## 9. Implementation roadmap

Each step is one reviewable change that builds, with its check. Guide rules it touches are named so review can hold the change to them.

| Step | Change                                                                                                                                           | Verify                                                                                                                                    |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| P1a  | `nyanpasu-paths`: `LogicalPath`, `PathClass`, `PathEntry` table, resolver methods for the eleven ad hoc joins; callers use them                  | unit tests for validation; "every path method is covered by one row"; `pnpm test:backend`                                                 |
| P1b  | `enumerate_sync_items`; `backup.rs::skipped_paths` derived from classes                                                                          | tempdir tests: dot entries, links, case collisions, selection; existing backup tests unchanged                                            |
| P1c  | Layout migration steps (`session_state_local`, `runtime_to_data`) with the core-manager path change                                              | migration tests on fixtures, including downgrade-then-upgrade; Unix socket path length check                                              |
| P2a  | Remove `cfg` from the config schemas: unconditional macOS field, `TrayMenuMode::Auto`, neutral defaults, `FreshInstallDefaults` from both shells | round-trip tests of files written by the previous release; regenerated bindings identical on all platforms; zero-`cfg` gate added         |
| P2b  | `nyanpasu-config::sync`: scope tables, projections, canonical encoding, `portable_profile_files`                                                 | exhaustiveness test; golden hashes checked on Linux, macOS and Windows CI; CRLF and BOM cases                                             |
| P2c  | `merge_document` and the catalog merge                                                                                                           | table-driven three-way tests per unit and policy; pass-through of unknown fields; idempotence (`merge(base, merged, remote) == merged`)   |
| P3a  | `nyanpasu-contracts::sync` ports and values; in-memory `RemoteStore` fake with switchable capabilities                                           | port contract tests run against the fake                                                                                                  |
| P3b  | `nyanpasu-sync-opendal` with `fs`, `webdav`, `s3`; boundary policy updates                                                                       | the same contract tests against OpenDAL `memory` and `fs`; error mapping tests; `pnpm run lint:backend-boundaries`                        |
| P4a  | `nyanpasu-core::sync`: commit format, pure planner, state store                                                                                  | planner tests for every row of [§6.4](#64-change-detection), [§6.6](#66-concurrency-across-devices) and [§6.8](#68-first-sync-and-rejoin) |
| P4b  | `SyncActor`, `SyncClient`, apply port, snapshots, profiles catalog-plus-files message                                                            | two simulated devices over one fake store; crash injection at each phase boundary; no sleeps                                              |
| P5   | Facade methods, RPC commands, status event, job registration, composition in `backend/tauri`                                                     | RPC tests through the registry; `unsupported` when `sync` is `None`; catalog and bindings regenerated                                     |
| P6   | Settings UI, conflict and snapshot views, i18n                                                                                                   | `pnpm typecheck`, `pnpm lint`, `pnpm test:frontend`; manual run against a real WebDAV server and an S3-compatible endpoint                |
| P7   | Later, separately reviewed: OAuth providers, encryption, `web:*` storage item, server and local-socket exposure, keychain secret store           | each with its own design note                                                                                                             |

P1 and P2 are useful without sync (one path classification, one cross-platform schema) and carry no OpenDAL dependency. OpenDAL enters the tree at P3b.

Rust rules in play: snafu error enums with structured fields, no `Custom(String)` (§7.2); ports owned by their consumer, `Arc<dyn Port>`, `async_trait`, mockable (§5.3); newtypes for `LogicalPath`, `CommitId`, `ObjectId`, `DeviceId`, `VaultId` (§5.2); new persisted fields default (§6); platform code on items in adapter modules (§9); no in-process timeouts, deadlines in the network adapter (architecture guide).

---

## 10. Test strategy

- **Pure first.** Projection, canonical encoding, merge and planning take and return plain values. They carry the protocol's logic and get table-driven tests with no IO.
- **Golden portability tests.** A fixed set of documents and files with expected SHA-256 values runs on all three CI platforms. A difference between platforms fails the build; this is the executable form of "strictly cross-platform".
- **Port contract suite.** One set of tests for `RemoteStore` semantics (`IfAbsent`, `NotFound`, recursive listing, large and small bodies, key validation) runs against the in-memory fake, OpenDAL `memory` and OpenDAL `fs` in a tempdir. No test needs the network.
- **Capability matrix.** The fake store can disable `create_if_absent`, ignore it silently, drop `last_modified`, delay listings and reorder them. The two-device tests run in both naming modes and under each fault.
- **Convergence property.** Random sequences of edits, deletions and syncs on two and three simulated devices must end with equal portable state on all devices once every device has synced twice with no further edits, and must never lose an edit under `keep_both`.
- **Crash injection.** The actor test stops a cycle after each phase and restarts with the same store and state; the resulting state must equal that of an uninterrupted cycle.
- **Owner integration.** In core's integration tests, with profile-io as a dev-dependency: a pulled catalog and its files commit in one transaction; a rejected merge leaves the document untouched; a local edit during a cycle defers the item.
- **Secrets.** Tests assert that no query DTO, log line or diagnostics archive contains a configured secret, and that `Custom.options` rejects secret keys.
- **Live smoke tests**, manual and outside CI: a WebDAV server (Nextcloud or `rclone serve webdav`) and MinIO, two real installs on different operating systems.

Tests use injected paths and tempdirs, typed clients with fake adapters, explicit acknowledgements instead of sleeps, and no global fixtures.

---

## 11. Alternatives considered

### 11.1 Physically splitting documents into portable and local files

A portable `application.yaml` beside a `local/application.yaml` override would make every synced item a plain file. It was rejected for the first design:

- one state commit would write two files, and the state engine's guarantee is one document, one atomic write;
- `profiles.yaml` carries `revision`, which materialization recovery compares with its journals; a commit torn between two files would make recovery misjudge which side landed;
- the files on disk would still need canonicalization before hashing (stamp, YAML formatting, key order), so the hashed bytes would be a projection anyway;
- remote changes must be applied through the owners in either design.

The scope tables make a later physical split mechanical if it is ever wanted, and they are the same tables.

### 11.2 Moving subscription downloads out of `profiles/`

Putting the files of `Remote` profiles under a cache directory would let enumeration ignore the catalog. It requires `ProfileFsPort` and the materialization transactions to resolve a path by source kind, and a file move when a profile changes kind. `SelectedTree` gets the same result with one pure function over the committed catalog.

### 11.3 A mutable index object with compare-and-swap

A single `index.json` updated with `if_match` is the textbook design. Only S3 and OneDrive offer `if_match` in OpenDAL 0.59.5, and WebDAV, the most common target for this application, offers no conditional write at all. Immutable commits work everywhere and make the conditional create an optimisation.

### 11.4 Remote lock files

A lock object needs either an atomic create (then it is unnecessary, the commit itself can be the atomic create) or a timeout (then two devices can hold it). It also turns a crashed client into a blocked vault.

### 11.5 Per-object metadata instead of a manifest

Listing every object with its etag or modification time costs one listing but depends on metadata several providers do not return, and cannot express a deletion or a consistent snapshot across items.

### 11.6 Syncing the config directory as files

Covered in [§1.4](#14-portability-hazards-a-naive-file-sync-would-hit): it moves device state between machines, breaks on absolute paths, and fights the owners.

## 12. Open questions

1. `current` is device-scoped here. If users expect the active profile to follow them, it can become an opt-in portable field; the `DeleteActive` rule stays either way.
2. Whether `enable_system_proxy`-style switches should have a "follow my other devices" option. The design says no; they are machine state.
3. Whether the first delivery should ship without encryption given that vaults hold subscription tokens. The alternative is to gate third-party providers (anything but a self-hosted WebDAV or S3 endpoint) on [§6.9](#69-reserved-client-side-encryption).
4. The headless server and OpenWrt: the port and protocol fit, but the binary-size budget on musl targets and the exposure of sync commands over the local socket need their own review.

---

## Appendix A: OpenDAL 0.59.5 probe

A scratch crate outside the repository, built with `default-features = false` and the features `executors-tokio`, `layers-retry`, `layers-timeout`, `services-{fs,memory,webdav,s3,dropbox,gdrive,onedrive}`, plus `opendal-http-transport-reqwest` and `reqwest 0.13`. Operators for the network services were built against an unreachable endpoint to read capabilities; `fs` and `memory` were exercised.

| Check                                                                                                               | Result                                                                                          |
| ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `Operator::via_iter("fs", ..)` with `auto-register-services` off                                                    | `Unsupported: scheme is not registered` (only `memory` is registered by `opendal-core`)         |
| `Operator::from_iter::<services::Fs>(..)` and the other six builders                                                | builds                                                                                          |
| `op.with_context(OperationContext::new().with_http_transport(HttpTransporter::new(ReqwestTransport::new(client))))` | compiles and runs with a caller-owned `reqwest::Client`                                         |
| `write_with(path, bytes).if_not_exists(true)` on an existing object, `fs` and `memory`                              | `ErrorKind::ConditionNotMatch`                                                                  |
| `write_with(..).if_match(..)` on `fs` and `memory`                                                                  | `ErrorKind::Unsupported`                                                                        |
| `stat` on `fs`                                                                                                      | `last_modified` present, `etag` and `content_md5` absent                                        |
| `stat` on `memory`                                                                                                  | no `etag`, no `last_modified`                                                                   |
| `list_with("").recursive(true)` on `fs`                                                                             | works although `list_with_recursive` is `false`; returns directories and the root entry as well |
| `stat` of a missing object                                                                                          | `ErrorKind::NotFound`                                                                           |
| Capability flags                                                                                                    | the table in [§5.5](#55-capability-heterogeneity)                                               |
| Release size                                                                                                        | the table in [§5.4](#54-features-and-build-footprint)                                           |

The probe is not committed. P3b recreates its assertions as the port contract suite, which then guards them against OpenDAL upgrades.

## Appendix B: Sources referenced

- Paths: `backend/nyanpasu-paths/src/{lib,unix,windows,registry,error}.rs`, `backend/nyanpasu-paths/README.md`
- Schemas: `backend/nyanpasu-config/src/application/{mod,logging}.rs`, `clash/config/{mod.rs,overrides/mod.rs,overrides/tun.rs}`, `profile/{profiles,source,definition,path,metadata,id}.rs`, `state/{mod,window}.rs`
- Persistence and migration: `backend/nyanpasu-persistence/src/{lib,write,format}.rs`, `backend/nyanpasu-migration/src/{lib,backup,store,registry}.rs`
- Profile files: `backend/nyanpasu-profile-io/src/{fs_service,materialization}.rs`, `backend/nyanpasu-contracts/src/profiles/ports.rs`
- Owners and facade: `backend/nyanpasu-core/src/state/{application,clash_config}.rs`, `state/profiles/actor.rs`, `client/{mod,jobs,runtime}.rs`, `actor_v2/local_host.rs`, `storage.rs`
- Composition: `backend/tauri/src/setup.rs`, `backend/tauri/src/core/tray/icon.rs`, `backend/nyanpasu-server/src/compose.rs`
- Standards: `docs/development/{architecture,backend-packages,rpc,rust,testing,workflow}.md`
- Related designs: `docs/design/2026-10-01-migration-backup.md`, `docs/design/2026-10-26-ui-state-change-events.md`, `docs/design/2026-10-03-app-updater.md`, `docs/design/jobs-subsystem.md`
