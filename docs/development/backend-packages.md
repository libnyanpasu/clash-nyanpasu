# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior
and concrete implementations live in separate neutral crates. See the
[extraction plan](../design/application-platform-extraction.md) for the
delivered split stack and the target ownership; this guide defines the current
dependency rules.

| Package                    | Owns                                                                                              | Must not depend on                                         |
| -------------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `nyanpasu-config`          | Domain models, profile validation, pure runtime executor and its ports                            | Tauri, egui/eframe or Slint                                |
| `nyanpasu-core`            | NyanpasuClient, use cases, typed actor clients, workflows, consumed ports and state transactions  | Platform implementation crate, Tauri, egui/eframe or Slint |
| `nyanpasu-platform`        | Transitional: concrete profile filesystem/HTTP, script, runtime-build and changelog HTTP adapters | Tauri host, egui/eframe or Slint                           |
| `nyanpasu-widget-protocol` | Render-free statistic widget contract: variants, messages and the optional `ipc` channel          | Tauri, egui/eframe or Slint                                |
| `nyanpasu-widget`          | Statistic widget process frontend, rendered with Slint                                            | Tauri; no neutral crate may depend on it                   |
| `nyanpasu-diagnose`        | Host facts, diagnostic reports, redacted archives and synchronous progress protocol               | Core, platform, Tauri, egui/eframe or Slint                |
| `nyanpasu-crash-reporter`  | Crash report window, rendered with Slint                                                          | Tauri; no neutral crate may depend on it                   |
| `backend/tauri`            | Desktop composition root, GUI adapters, lifecycle, IPC and HTTP transports                        | No reverse dependency from neutral crates                  |

The core crate receives dependencies through constructors, function
arguments or actor startup parameters. Platform can depend on core to
implement its ports. Core must not import platform in production. Keep
cross-layer tests in platform or host tests rather than creating a dependency
cycle. Tauri-specific adapters remain in the host, outside neutral platform.

## Target packages

`nyanpasu-platform` currently mixes script runtimes and portable infrastructure,
while OS integration code sits in core and in the Tauri crate. The target
ownership in the [extraction
plan](../design/application-platform-extraction.md#target-ownership) splits
them. Names are working names until the PR that creates each crate.

| Package (planned)            | Owns                                                                                                                                  | Depends on                                                  |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `nyanpasu-profile-transform` | Runtime builder, builtin transforms and artifact snapshots, moved from `nyanpasu-core::enhance`                                       | `nyanpasu-config`; not core, not the script crate           |
| `nyanpasu-scripts`           | JavaScript and Lua runners and their extension definitions, implementing the config `ScriptRunner` port                               | `nyanpasu-config` ports only                                |
| `nyanpasu-os`                | OS integration: system proxy, PAC, autostart, service install and control, DNS flush, device identity, core binary install, elevation | Core or contract ports; not Tauri or presentation libraries |
| `nyanpasu-io`                | Portable infrastructure: profile files, subscription and changelog HTTP, egress probes, updater downloads, watchers, storage backends | Core or contract ports; renamed from `nyanpasu-platform`    |

Presentation capabilities (hotkeys, windows, tray, widgets, dialogs,
notifications, clipboard, opener) have no shared implementation crate. Each
host implements them, and core treats them as optional capabilities with an
explicit unsupported outcome.

Until a target crate exists:

- Do not add script runtime or OS integration code to `nyanpasu-platform`.
- Do not add a mandatory presentation capability to `ClientSetupArgs`.
- Keep new OS integration code behind a consumer-owned port next to its current
  consumer, so moving it later does not change callers.

Domain configuration must not import a GUI package merely for a wire enum. Put
such a type in an appropriate neutral owner and preserve its schema and public
type identity. Move implementations and migrate callers together; do not keep
old-path wrappers solely to avoid updating imports.

`NyanpasuClient`, its actor/workflow graph and profile/configuration
transactions live in core. The desktop composition root in
`backend/tauri/src/setup.rs` injects platform implementations and GUI adapters.
Preserve commit/vote/compensation and owner lifecycle semantics when changing
these call paths.

Run `deno task lint:backend-boundaries` after changing backend package direction.
It inspects resolved production/build dependencies for all targets, including
transitive GUI leaks. Its source is in the architecture-ledger script category;
`deno task test:scripts` covers its policy tests. Run relevant Rust behavior tests
and desktop consumer checks as well: a dependency gate does not prove behavior.

## Consolidated runtime baseline

The runtime builder, builtin transforms and script descriptor types live in
`nyanpasu-core::enhance`, alongside the facade and its dependent actor/workflow
graph. Platform's `enhance::runtime_build` implements the core-owned
`RuntimeBuildPort`; `profiles::fs_service` implements profile file access, and
`changelog_http` implements changelog fetching. Platform and desktop consumers
import core directly. The former `nyanpasu-application` crate and its
workspace/dependency entries have been removed.

This is a baseline, not the target. The runtime builder moves to
`nyanpasu-profile-transform`, and the script runners move out of platform into
the script crate. `FsRuntimeBuildAdapter` will receive its script runner from
the host instead of constructing it.
