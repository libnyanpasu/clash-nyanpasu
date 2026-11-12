# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior
and concrete implementations live in separate neutral crates. See the
[extraction plan](../design/application-platform-extraction.md) for the
delivered split stack and the target ownership; this guide defines the current
dependency rules.

| Package                      | Owns                                                                                                | Must not depend on                                                                                                            |
| ---------------------------- | --------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `nyanpasu-config`            | Domain models, profile validation, pure runtime executor and its ports                              | Tauri, egui/eframe or Slint                                                                                                   |
| `nyanpasu-contracts`         | Bundle/profile/network contracts, OS ports and errors, core version and staged install requests     | Core, transform, implementation crates, `ractor`, `reqwest`, `redb`, core manager, local channel, Tauri, egui/eframe or Slint |
| `nyanpasu-profile-transform` | Runtime builder, builtin transforms and post-processing output types over config's executor         | Core, contracts, implementation crates, Tauri, egui/eframe or Slint                                                           |
| `nyanpasu-persistence`       | Generic filesystem retry and evidence-preserving recovery                                           | Core, contracts, transform, implementation crates, `redb`, core manager, Tauri, egui/eframe or Slint                          |
| `nyanpasu-profile-io`        | Profile filesystem access, materialization transactions and content source                          | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-net`               | Subscription, changelog, direct-egress, icon and diagnostics HTTP adapters and client policy        | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-os`                | Proxy, PAC, autostart, device identity, version probe, service, DNS flush and core install          | Core family, runtime IPC, core manager, other implementation crates, Tauri, egui/eframe or Slint                              |
| `nyanpasu-owner-support`     | Owner shutdown tracking and blocking-task result handling                                           | Other core-family packages, implementation crates, Tauri, egui/eframe or Slint                                                |
| `nyanpasu-observers`         | Core-log, traffic and country-index owners over injected store, frame and mode ports                | Implementation crates, `redb`, `clash-api`, core manager, `notify`, `notify-debouncer-full`, Tauri, egui/eframe or Slint      |
| `nyanpasu-migration`         | Configuration upgrades, backups and session document reads through storage ports                    | Implementation crates, `redb`, core manager, Tauri, egui/eframe or Slint                                                      |
| `nyanpasu-updater`           | Core update owner, artifact preparation, downloads and unpacking over an injected backend factory   | Implementation crates, Tauri, egui/eframe or Slint                                                                            |
| `nyanpasu-rpc`               | Shared commands, registry, error conversion, audit kernel/sink, event bus and optional HTTP adapter | GUI and implementation crates; foundation, core-family and implementation crates must not depend on RPC                       |
| `nyanpasu-server`            | Headless composition and resident executable, with explicit refusal of elevation                    | Tauri, egui/eframe, Slint and their transitive GUI dependencies                                                               |
| `nyanpasu-rpc-protocol`      | RPC envelopes, owner/event values, command metadata/catalog, local operation contracts and client   | GUI, every core-family member (including state-engine) and implementation crates                                              |
| `nyanpasu-runtime-values`    | Public mutation/error values, status/update snapshots, storage values and hotkey vocabulary         | Core family, engine, implementation crates, GUI and core manager                                                              |
| `nyanpasu-state-engine`      | Generic state coordination, persistence, snapshots and transaction settlement                       | Every parent-workspace package except persistence; other core-family packages and GUI                                         |
| `nyanpasu-core`              | NyanpasuClient, use cases, typed actor clients, workflows, consumed ports and source state owners   | Implementation crates, Tauri, egui/eframe or Slint                                                                            |
| `boa_utils`                  | Boa module parsing and builtins over injected HTTP source ports                                     | Core family, implementation crates and GUI                                                                                    |
| `nyanpasu-scripts`           | JavaScript and Lua script runners behind config's `ScriptRunner`                                    | Core, other implementation crates, Tauri, egui/eframe or Slint                                                                |
| `nyanpasu-widget-protocol`   | Render-free statistic widget contract: variants, messages and the optional `ipc` channel            | Tauri, egui/eframe or Slint                                                                                                   |
| `nyanpasu-widget`            | Statistic widget process frontend, rendered with Slint                                              | Tauri; no neutral crate may depend on it                                                                                      |
| `nyanpasu-diagnose`          | Host facts, diagnostic reports, redacted archives and synchronous progress protocol                 | Core, Tauri, egui/eframe or Slint                                                                                             |
| `nyanpasu-crash-reporter`    | Crash report window, rendered with Slint                                                            | Tauri; no neutral crate may depend on it                                                                                      |
| `backend/tauri`              | Desktop composition root, GUI adapters, lifecycle, native IPC and HTTP composition                  | No reverse dependency from neutral crates                                                                                     |

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
keeps facade, application and core control as modules in `nyanpasu-core`, above
the delivered owner, foundation and implementation crates. D1 delivered the
state-engine leaf, and D2 delivered the upper-foundation runtime-values package
and the source/workflow module boundary.
Names are working names until the PR that creates each crate.

| Group                                      | Packages (delivered unless marked planned)                                                                                                     | Depends on                                                                                                                                              |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shells                                     | `backend/tauri` (desktop GUI), `nyanpasu-server` (headless), mobile shell (later)                                                              | Anything below; only shells construct `NyanpasuClient`                                                                                                  |
| CLI                                        | `nyanpasu-cli`, a client of a running shell over the local socket                                                                              | `nyanpasu-rpc-protocol` and the local-socket client; not core                                                                                           |
| Protocol (base delivered E1a-b)            | `nyanpasu-rpc-protocol`                                                                                                                        | Approved foundation values, including the delivered runtime-values package; no GUI, CORE_FAMILY or IMPLEMENTATION                                       |
| RPC adapter (E2b delivered; local planned) | `nyanpasu-rpc`, HTTP and local-server features                                                                                                 | Core and protocol; no GUI or IMPLEMENTATION; foundation/core/implementation crates must not depend on this adapter                                      |
| Target crates                              | `nyanpasu-os` (OS integration), `nyanpasu-openwrt` (on resume)                                                                                 | The foundation layer only; never core or other implementation crates                                                                                    |
| Core package family                        | `nyanpasu-core` (facade, application and control modules), migration, owner-support, observers, updater, `nyanpasu-state-engine`               | Downward to foundation; engine is a leaf and may depend only on persistence among parent-workspace packages                                             |
| Foundation leaves                          | `nyanpasu-config`, `nyanpasu-contracts`, `nyanpasu-persistence`, `nyanpasu-profile-transform`                                                  | Existing leaf rules remain; none may depend on runtime-values                                                                                           |
| Upper foundation (delivered D2)            | `nyanpasu-runtime-values`: public mutation results, degradation, runtime/profile error closure                                                 | Config (defaults off), contracts, transform, neutral core metadata and required libraries; no CORE_FAMILY, engine, implementations, GUI or core manager |
| Implementation crates                      | `nyanpasu-scripts` (JS and Lua runners), `nyanpasu-profile-io` (profile files), `nyanpasu-net` (subscription and other HTTP), storage backends | The foundation layer only; never core, never each other                                                                                                 |

E1a-b delivered protocol owner values, request/error envelopes, transparent JSON and the eleven event identities. Cookie parsing, HTTP status mapping and application error conversion live in `nyanpasu-rpc`; protocol consumes approved foundation values alongside Serde/JSON/Specta and Snafu, with its frame IO behind the default-off `frame-io` feature delivered in E3b. The remaining Phase E work stays planned; [tasks §5](../spec/2026-11-03-core-package-split/tasks.md#5-phase-e-的-pr-划分) defines its implementation cards. The protocol `types` module owns the receiving error/mutation graph, owned error envelope, connection details and `CoreLogsChanged` payload for the thirteen local operations and their five full-value snapshots; it owns no application error downcasts or operational sources. Ordinary values are canonical definitions in foundation crates, shared by the application and protocol without projections. A separate local-client crate is deferred unless `protocol/local-client` cannot isolate the client dependency closure.

The RPC adapter owns thin shared commands and the event bus. Its `registry` module owns dispatch, `audit` owns audit instances, owners, sinks, transports, records, outcomes and the record guard, and `ports` owns the host facilities consumed by shared commands and registry dependencies. Audit does not depend on registry, commands or transports; ports does not depend on commands or registry, and registry does not depend on commands. Bus-first event delivery remains E2c work. Tauri IPC/native channels and asset resolution remain shell adapters; HTTP and local-server are explicit registry features. Local client framing/decoding is a protocol feature with no registry dependency. Missing required capabilities leave known commands registered and return `unsupported`, not `unknown_method`.

E1a-c adds neutral command target/capability values and the generated production
command catalog to protocol. The catalog retains conditional declarations and
known native names independently of linked handlers. Its scoped extractor runs
only as explicit development tooling in the macro tests; protocol has no normal
or build dependency on the macro crate, application declarations or core. Macro
dependencies remain syntax/code-generation tools. Shared handlers consume the
explicit dependency/caller interface in `nyanpasu-rpc::registry`;
native extraction remains a separate shell wrapper. See the [catalog workflow](rpc.md#neutral-declarations-and-native-wrappers).

E1a-d supplies desktop capabilities explicitly from the composition root and
checks required facilities and target opt-in before argument parsing or handler
execution. The registry keeps every known catalog name, including unlinked native
commands, and audits `unsupported` rejections; duplicate handlers fail regardless
of capability availability. Optional metadata describes reachable effects without
denying unrelated operations. This is admission metadata, not per-call authority
or an elevation policy. Existing HTTP exposure is unchanged, and production local
opt-ins remain deferred to E3e. No package edges or boundary collections change.

The server is a composition shell with no GUI/Tauri dependency. Its first composition injects `RefuseElevation` into service control, DNS flush and binary installation, keeps shared effects and supplies neutral sinks without presentation capabilities. This changes only the separate server; the embedded desktop WebUI retains the desktop policy. CLI production normal/build closure forbids CORE_FAMILY (including state-engine), IMPLEMENTATION, RPC adapter, server and GUI; foundation values reached through protocol are allowed. Test-only server fixtures do not authorize a production edge. Package creation must update dependency policy tests, static-prefix coverage and both musl package/expected-target lists together.

The server packager or operator must provision geodata (`Country.mmdb`,
`geoip.dat`, `geosite.dat`) in the application data directory selected by the
injected `PathResolver`, and on Windows provide `wintun.dll` where the selected
core can load it. The server does not copy desktop bundled resources at startup.
Missing geodata leaves country lookup unavailable and can cause core startup or
configuration application to fail when the selected configuration needs those
files; missing Wintun can prevent Windows TUN startup. These files are not required
merely to assemble the server or bind its listeners.

Phase D boundaries are defined in [tasks §4](../spec/2026-11-03-core-package-split/tasks.md#4-a2b-与-phase-d-的-pr-划分). `nyanpasu-state-engine` enforces the generic transaction dependency boundary, not a compile-time claim. Its only parent-workspace dependency is `nyanpasu-persistence`, with default features disabled; dependencies brought by persistence from the separate runtime workspace do not authorize new direct engine dependencies. The gate rejects every other core-family member and every other parent-workspace package. The original foundation leaves explicitly reject the new upper-foundation values package, which in turn rejects core-family/implementation/GUI/core-manager dependencies.

Control modules are `actor_v2/`, `clash/`, `proxies.rs` and `connections.rs`. Application code is `client/` (facade, workflow, lifecycle and effects); source owners remain under `state/`. Application may call control; control must not import `client`. Source owners must not directly import `client::application_workflow` or its concrete `ApplicationWorkflowClient`; they consume `MutationParticipantFactory<T>` from `state/mutation/ports.rs`, implemented by the workflow participant. Internal `MutationReceipt` and `RuntimeApplyReceipt` stay in core's `state/mutation/`, not runtime-values. `CommitNotifications` and its effect-field values remain in `client/effects`; `EffectKind` lives in `nyanpasu-runtime-values::configuration`. Source dependencies on effects, jobs and value/validation contracts remain allowed. This is not a ban on every `state → client` edge or a claim of transitive module independence.

D2f delivered a bounded syntactic source guard beside the Cargo gate: explicit grouped/aliased use trees, relative/qualified paths and lexical module/block scopes, including tests. It indexes ordinary same-core provider modules and follows explicit aliases/re-exports for at most eight hops; cycles, ambiguity, unresolved providers and unsupported forms fail explicitly. `self::*` and `super::*` are allowed only when the named module is itself policed; inherited alias bindings are resolved recursively at their declaration scope. Other globs, including provider re-export globs, are rejected, as is `r#` anywhere in a checked path. It does not perform Rust type resolution, macro expansion or call-graph analysis. Allowed local service bridges and external `nyanpasu_ipc::client` have positive fixtures; indirect forbidden re-exports have negative fixtures. `proxies.rs` imports public results directly from runtime-values. Service compatibility outside `client` remains a permitted control dependency. No control/application crate is planned: the server needs both, the CLI neither. An independent control consumer would require a new closure inventory and review.

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

A2b delivered shell-injected script extensions, including HTTP/cache, console and
Lua library configuration. JavaScript and Lua features default on in scripts;
shells disable dependency defaults and select engines explicitly. Extension
factory methods decline by default: execution requires both the scripts engine
feature and shell-provided extensions, otherwise it returns typed `StartEngine`
through the pipeline and wire. This preserves optional shell dependencies under
Cargo feature unification. Determinism assumes equal injected network, file and
clock observations; it does not promise unconditional replay.

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

The planned E packages land only after D2 → D3 → D4 → A2b, in E1a → E2 → E3 → E1b → E4 order. The RPC adapter owns the neutral production `TracingRpcAuditSink` alongside the registry/kernel and bus. Its planned E3 local subscription adapter admits only five full-value status identities and the separate connection-detail demand stream (tasks E3b); invalidation/storage recovery is deferred to a separately reviewed card, with no new package or production edge. Shells inject host facilities and synchronous native delivery; SSE/local use bounded asynchronous receivers. Protocol local-client owns its connection through a dedicated transport task. The server installs its process panic policy before task startup; none of these ownership changes adds a dependency from lower layers to RPC.

Planned E2a closes shared signatures before extraction: four response definitions follow the commands into RPC, while `TrayIcon` is a protocol value with shell asset functions. E2b unconditionally includes the existing runtime-values package in dependency policy. E4a adds default-off `nyanpasu-os/test-support` exposing only real-adapter test constructors and an intercepting runner; only the server dev-dependency enables it. Production constructors retain the OS runner. The gate resolves an independent production consumer to reject direct and transitive normal/build feature leaks, separately from test feature unification. E4b adds the `nyanpasu-server` binary beside the library, with both identities required by musl artifact collection. Package rules and static prefixes are unchanged. The executable owns the process panic boundary, local listener and optional authenticated loopback HTTP listener over the same client.

## Phase A result

The [core package split spec](../spec/2026-11-03-core-package-split/design.md)
Phase A is delivered: runtime-build orchestration and artifact conversion live in
core; runtime transforms, script runners, generic persistence, profile IO and HTTP
adapters have separate owners. Application integration tests live in core with
profile-io and net as dev-dependencies. Transform's golden and real-engine tests
use scripts as a dev-dependency. Tauri owns its test assembly, and the transitional
test crate has been removed. The remaining core family, target and shell work is
described in phases B–E of the spec.

E2b moves the neutral production `TracingRpcAuditSink` with the registry and HTTP
actor. The desktop enables `nyanpasu-rpc/http` explicitly; HTTP is off by default.
Core/config dependencies disable defaults and forward `default-meta` and `nesting`.
The existing Tauri-to-bus bridge and required frontend assets remain until E2c/E3a.
RPC test support uses injected core fixtures; it has no implementation-crate edge.
