# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior
and concrete implementations live in separate neutral crates. See the
[extraction plan](../design/application-platform-extraction.md) for the
delivered split stack and the target ownership; this guide defines the current
dependency rules.

| Package                      | Owns                                                                                              | Must not depend on                                                                                                            |
| ---------------------------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `nyanpasu-config`            | Domain models, profile validation, pure runtime executor and its ports                            | Tauri, egui/eframe or Slint                                                                                                   |
| `nyanpasu-contracts`         | Bundle/profile/network contracts, OS ports and errors, core version and staged install requests   | Core, transform, implementation crates, `ractor`, `reqwest`, `redb`, core manager, local channel, Tauri, egui/eframe or Slint |
| `nyanpasu-profile-transform` | Runtime builder, builtin transforms and post-processing output types over config's executor       | Core, contracts, implementation crates, Tauri, egui/eframe or Slint                                                           |
| `nyanpasu-persistence`       | Generic filesystem retry and evidence-preserving recovery                                         | Core, contracts, transform, implementation crates, `redb`, core manager, Tauri, egui/eframe or Slint                          |
| `nyanpasu-profile-io`        | Profile filesystem access, materialization transactions and content source                        | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-net`               | Subscription, changelog, direct-egress, icon and diagnostics HTTP adapters and client policy      | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-os`                | Proxy, PAC, autostart, device identity, version probe, service, DNS flush and core install        | Core family, runtime IPC, core manager, other implementation crates, Tauri, egui/eframe or Slint                              |
| `nyanpasu-owner-support`     | Owner shutdown tracking and blocking-task result handling                                         | Other core-family packages, implementation crates, Tauri, egui/eframe or Slint                                                |
| `nyanpasu-observers`         | Core-log, traffic and country-index owners over injected store, frame and mode ports              | Implementation crates, `redb`, `clash-api`, core manager, `notify`, `notify-debouncer-full`, Tauri, egui/eframe or Slint      |
| `nyanpasu-migration`         | Configuration upgrades, backups and session document reads through storage ports                  | Implementation crates, `redb`, core manager, Tauri, egui/eframe or Slint                                                      |
| `nyanpasu-updater`           | Core update owner, artifact preparation, downloads and unpacking over an injected backend factory | Implementation crates, Tauri, egui/eframe or Slint                                                                            |
| `nyanpasu-core`              | NyanpasuClient, use cases, typed actor clients, workflows, consumed ports and state transactions  | Implementation crates, Tauri, egui/eframe or Slint                                                                            |
| `nyanpasu-scripts`           | JavaScript and Lua script runners behind config's `ScriptRunner`                                  | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-widget-protocol`   | Render-free statistic widget contract: variants, messages and the optional `ipc` channel          | Tauri, egui/eframe or Slint                                                                                                   |
| `nyanpasu-widget`            | Statistic widget process frontend, rendered with Slint                                            | Tauri; no neutral crate may depend on it                                                                                      |
| `nyanpasu-diagnose`          | Host facts, diagnostic reports, redacted archives and synchronous progress protocol               | Core, Tauri, egui/eframe or Slint                                                                                             |
| `nyanpasu-crash-reporter`    | Crash report window, rendered with Slint                                                          | Tauri; no neutral crate may depend on it                                                                                      |
| `backend/tauri`              | Desktop composition root, GUI adapters, lifecycle, IPC and HTTP transports                        | No reverse dependency from neutral crates                                                                                     |

Contracts and persistence must not depend on each other. Persistence and profile
transform must not depend on each other either.

The core crate receives dependencies through constructors, function
arguments or actor startup parameters. Implementation crates depend on the
foundation layer only, never on core or each other. Core must not depend on an
implementation crate in production. The core family must not depend on `sysproxy`
or `runas`; OS proxy and elevation mechanisms belong to `nyanpasu-os`. Application integration tests live in core
with implementation crates as dev-dependencies; host test assembly stays in the
host. Core's exported test support accepts injected adapters without linking
implementation crates. Tauri-specific adapters remain in the host.

## Target packages

The [target
ownership](../design/application-platform-extraction.md#target-ownership)
splits `nyanpasu-core` into a package family above the delivered foundation and
implementation crates.
Names are working names until the PR that creates each crate.

| Group                 | Packages (planned)                                                                                                                             | Depends on                                                                                             |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Shells                | `backend/tauri` (desktop GUI), `nyanpasu-server` (headless), mobile shell (later)                                                              | Anything below; only shells construct `NyanpasuClient`                                                 |
| CLI                   | `nyanpasu-cli`, a client of a running shell over the local socket                                                                              | `nyanpasu-rpc-protocol` and the local-socket client; not core                                          |
| RPC                   | `nyanpasu-rpc-protocol`, `nyanpasu-rpc`, the WebUI (HTTP) and local-socket transports                                                          | Protocol: config and contracts. Registry: core                                                         |
| Target crates         | `nyanpasu-os` (OS integration), `nyanpasu-openwrt` (on resume)                                                                                 | The foundation layer only; never core or other implementation crates                                   |
| Core package family   | `nyanpasu-core` facade, application, core control, log/traffic/geodata owners, updater, state engine, migration                                | Downward only, ending at the foundation layer                                                          |
| Foundation layer      | `nyanpasu-config`, `nyanpasu-contracts`, `nyanpasu-persistence` (formats, file recovery), `nyanpasu-profile-transform` (runtime builder)       | Lower foundation crates only; contracts, persistence and transform depend on config, not on each other |
| Implementation crates | `nyanpasu-scripts` (JS and Lua runners), `nyanpasu-profile-io` (profile files), `nyanpasu-net` (subscription and other HTTP), storage backends | The foundation layer only; never core, never each other                                                |

Presentation capabilities (hotkeys, windows, tray, widgets, dialogs,
notifications, clipboard, opener, deep links, single instance) belong to the
shell. Their mechanisms stay in the Tauri shell: the hotkey owner, main-thread
handoff and presentation-effect execution in `backend/tauri/src/desktop/`, the
concrete GUI adapters in `backend/tauri/src/adapters/`. There is no desktop
interaction crate: the server needs none of these mechanisms, and new crate
boundaries brought no desktop compile-time gain. Core owns the ports it
consumes, hotkey validation before commit, hotkey action dispatch and the shared
convergence of effects (revisions, status, receipts). No core-family package
depends on the widget protocol directly; config uses its render-free widget
variant and re-exports it.

Core treats window control, hotkey validation, presentation-effect execution
and the debug HTTP server as optional. Without one, an operation that needs it
reports unsupported, an explicit hotkey edit is rejected before it is saved,
and a desired presentation effect is reported unsupported, never applied.
Sinks that only observe stay required: `UiEventSink` and `FrontendLogSink`
(frontend diagnostics written into the backend log); a shell without a GUI
passes a neutral implementation.

Until a target crate exists:

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
`backend/tauri/src/setup.rs` injects concrete implementations and GUI adapters.
Preserve commit/vote/compensation and owner lifecycle semantics when changing
these call paths.

Run `pnpm run lint:backend-boundaries` after changing backend package direction.
It inspects resolved production/build dependencies for all targets, including
transitive GUI leaks. Its source is in the architecture-ledger script category;
`pnpm run test:scripts` covers its policy tests. Run relevant Rust behavior tests
and desktop consumer checks as well: a dependency gate does not prove behavior.

## Phase A result

The [core package split spec](../spec/2026-11-03-core-package-split/design.md)
Phase A is delivered: runtime-build orchestration and artifact conversion live in
core; runtime transforms, script runners, generic persistence, profile IO and HTTP
adapters have separate owners. Application integration tests live in core with
profile-io and net as dev-dependencies. Transform's golden and real-engine tests
use scripts as a dev-dependency. Tauri owns its test assembly, and the transitional
test crate has been removed. The remaining core family, target and shell work is
described in phases B–E of the spec.
