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

The [target
ownership](../design/application-platform-extraction.md#target-ownership)
replaces `nyanpasu-platform` and splits `nyanpasu-core` into a package family.
Names are working names until the PR that creates each crate.

| Group                 | Packages (planned)                                                                                                           | Depends on                                                                 |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Shells                | `backend/tauri` (desktop GUI), `nyanpasu-server` (headless), mobile shell (later)                                            | Anything below; only shells construct `NyanpasuClient`                     |
| CLI                   | `nyanpasu-cli`, a client of a running shell over the local socket                                                            | `nyanpasu-rpc-protocol` and the local-socket client; not core              |
| RPC                   | `nyanpasu-rpc-protocol`, `nyanpasu-rpc`, the WebUI (HTTP) and local-socket transports                                        | Protocol: config and contracts. Registry: core                             |
| Target crates         | `nyanpasu-desktop` (desktop interaction), `nyanpasu-os` (OS integration), `nyanpasu-openwrt` (on resume)                     | Contracts and config; never core. The server never uses `nyanpasu-desktop` |
| Core package family   | `nyanpasu-core` facade, application, core control, log/traffic/geodata owners, updater, state engine, migration, persistence | Downward only, ending at `nyanpasu-contracts`                              |
| Profile enhancement   | `nyanpasu-profile-transform` (runtime builder), `nyanpasu-scripts` (JS and Lua runners)                                      | Config; the transform does not depend on the script crate                  |
| Implementation crates | `nyanpasu-profile-io`, `nyanpasu-net`, storage backends                                                                      | Contracts and config; never core                                           |

Presentation capabilities (hotkeys, windows, tray, widgets, dialogs,
notifications, clipboard, opener, deep links, single instance) belong to the
shell. A Tauri-free mechanism that several desktop shells can share goes into
the desktop interaction crate; a Tauri-bound one stays in `backend/tauri`. Core
treats presentation capabilities as optional, with an explicit unsupported
outcome.

Until a target crate exists:

- Do not add script runtime, OS integration or interaction code to
  `nyanpasu-platform`.
- Do not add a mandatory presentation capability to `ClientSetupArgs`.
- Keep new OS integration code behind a consumer-owned port next to its current
  consumer, so moving it later does not change callers.
- Expose a new command to WebUI (`rpc(http)`) only after reviewing its effects
  for a browser caller; CLI exposure will be a separate opt-in.

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
`nyanpasu-profile-transform`, the script runners move out of platform into
the script crate, and artifact-to-snapshot conversion stays in the application
layer. `FsRuntimeBuildAdapter` will receive its script runner from the shell
instead of constructing it.
