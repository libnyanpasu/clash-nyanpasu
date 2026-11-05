# OpenWrt downstream roadmap

Status: resumed for exploration by user, 2026-11-04, after being paused on
2026-10-08. The user judged the Tauri decoupling sufficient to resume. Baseline
reassessment is done and headless feasibility is in progress; see
[Reassessment, 2026-11-04](#reassessment-2026-11-04). Exploration does not
authorize the later milestones. No monitoring automation is requested.

This replaces the earlier OpenWrt-driven extraction sequence. The
[MVP definition](openwrt-mvp.md) describes the intended product outcome;
[development standards](../development/README.md) govern implementation.

## Dependency policy

OpenWrt is a downstream host over the shared application, not the driver of the
ongoing backend/shell migration. Let that migration settle its package ownership,
ports, bootstrap, lifecycle, persistence and observation contracts first.
Do not create an OpenWrt-specific facade, persistence protocol or compatibility
layer to bypass unfinished upstream work. Do not reorganize or extend upstream
PRs merely to implement this roadmap.

NyanpasuClient, shared use cases, typed clients, actors, workflows and consumed
ports live in `nyanpasu-core`, which the
[target ownership](application-platform-extraction.md#target-ownership) splits
into a package family without a second facade. Concrete adapters live in
implementation and target crates injected by the shell; core must not depend on
them or on a frontend. Config retains its domain models and pure executor.
Tauri remains the desktop shell; the router uses `nyanpasu-server` and
`nyanpasu-cli`.

The runtime behavior delivered by #5652 now lives in `nyanpasu-core::enhance`,
with platform/desktop consumers migrated and the former application crate removed.
Follow-up actor/facade extraction targets core directly. This is shared-backend
consolidation, not resumed OpenWrt work. OpenWrt still waits for upstream core/shell
completion and introduces no second application facade.

On resume, map the router to the actual accepted core APIs on main. Headless
construction and operation are readiness evidence; a renamed crate alone is not.

## Upstream dependency snapshot

Snapshot refreshed on 2026-11-04 from local refs; `git fetch` failed that day,
so remote branch state may be older.

| Work                                                                                        | State                                                                                                                                                                 | Treatment                                                                                                                         |
| ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| [#6106](https://github.com/libnyanpasu/clash-nyanpasu/pull/6106)                            | Merged: client and transaction graph moved into `nyanpasu-core`                                                                                                       | Delivered. The router composes this facade; no second facade.                                                                     |
| Core package split, phase A/B (`refactor/core-split-*`, through `core-split-b1b-migration`) | In review, not on main. Adds `nyanpasu-contracts`, `-scripts`, `-profile-transform`, `-net`, `-persistence`, `-profile-io`, `-migration`; retires `nyanpasu-platform` | The spike is based on `core-split-b1b-migration` by user decision, so it uses the final crate names. Rebase when the split lands. |
| RPC registry outside Tauri, `rpc(local)` CLI target                                         | Planned in [application-platform extraction](application-platform-extraction.md); the registry and generated code still use Tauri types                               | Required before native management. Do not add a router-only transport.                                                            |
| [#5645](https://github.com/libnyanpasu/clash-nyanpasu/pull/5645)                            | Merged frontend-independent path resolution                                                                                                                           | The spike uses `PathResolver::with_base_dirs` for explicit router roots.                                                          |

Router backend, the CLI, procd endpoints, LuCI/ACL, feed packaging and
OpenWrt-specific SDK acceptance remain unscheduled. There is no date estimate or
active OpenWrt PR queue.

## Resume gate

First establish that the relevant upstream shell/Tauri separation work has
completed and landed on main. Then verify the following against that baseline;
a merged PR count alone is insufficient.

- NyanpasuClient and its shared application APIs in nyanpasu-core support profile import/selection, script/runtime build,
  persistence, core control, status/receipts, recovery and orderly shutdown.
- The shared graph can be constructed using ordinary async/Tokio and injected
  paths/adapters, without linking Tauri/egui, starting a desktop shell, or
  supplying fake windows/tray/widgets/frontend assets.
- Shell/Tauri owns presentation, desktop bootstrap and transport integration.
  Shared business effects and frontend observations have explicit boundaries;
  unavailable capabilities do not report successful no-op application.
- Shared transactions/materialization preserve voting, commit, compensation,
  degradation, caller-drop and restart behavior. The desktop consumes the same
  accepted implementation and its relevant tests/CI pass.
- Neutral dependency gates and the affected desktop integration are verified on
  the chosen main revision. Actual Linux/musl/native-library portability remains
  a separate router validation step, not inferred from headless construction.

If these checks expose unfinished upstream work, report the missing capability
and keep OpenWrt paused instead of filling the gap with a router-only workflow.
The scope of shell/Tauri completion follows the upstream project; this roadmap
neither accelerates it nor reduces it to a few OpenWrt-required file moves.

## Work after explicit resume

| Milestone             | Work                                                                                                         | Exit evidence                                                                                                                                                            |
| --------------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Baseline reassessment | Refresh main/PRs and the completed shell architecture; map each MVP operation to accepted APIs and ports     | A concrete dependency/API mapping, target SDK/release tuples and revised implementation units; no duplicate application behavior.                                        |
| Headless feasibility  | Compose a minimal router host using the accepted shared graph; verify target dependencies and resource costs | Import/select/build/persist/core control/shutdown through real shared APIs; dependency tree excludes GUI; JS/Lua size/RSS and generic CPU/musl risks measured.           |
| Router backend        | Implement router paths/storage wiring, fixed procd control and lifecycle recovery                            | Real Mihomo validation, start/stop/status; durable profiles/scripts; preserved transaction failure semantics; crash policy tested.                                       |
| Native management     | Add the CLI allowlist over the local socket, then LuCI through rpcd with per-method ACL                      | CLI calls plus authenticated LuCI loop; unauthorized mutations denied; errors, pending actions and status observable.                                                    |
| Package acceptance    | Target-specific SDK/feed builds and installation in the chosen test environment                              | Both aarch64/x86_64 results recorded; native library ABI, package install, procd and LuCI verified. Physical-router support remains unclaimed without hardware evidence. |

These are future milestones, not fixed PR counts or frozen interfaces. Split
reviewable functional units after reassessment; keep each independently buildable.
Do not assume the previous eight-method DTO table, rpc(ubus) macro proposal,
filesystem layout, service names or stop/start convention are accepted contracts.
Select the thinnest adapter consistent with the completed shared application.

## Reassessment, 2026-11-04

The user resumed exploration on 2026-11-04 and chose to base it on
`refactor/core-split-b1b-migration` rather than main. The headless spike lives
on `feat/openwrt-headless-spike` as `backend/nyanpasu-server`. It is
feasibility evidence, not the production server. Places where core offers no
honest headless answer are marked `SPIKE: finding Fn` in its source.

### Operation mapping

| MVP step                    | Shared API                                                                                                                      | Status                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Import a local YAML profile | `create_profile(NewProfileRequest, initial_file)` with a managed local source                                                   | Works. No import-from-path/bytes helper; the caller builds `ProfileDefinition`.       |
| Ordered JS/Lua transforms   | Each script is a `Transform(Script)` profile; attach with `replace_profile_definition` (per profile) or `set_global_transforms` | Works. Order is the `transforms` list; no attach/reorder helper.                      |
| Select                      | `activate_profile(Some(id))`                                                                                                    | Works; the commit builds, checks and applies through the shared transaction.          |
| Build, inspect, validate    | Built and checked (`mihomo -t`) inside the mutation or `reconcile_core`; inspect with `runtime_yaml`, `inspect_runtime`         | No build- or validate-only operation. `runtime_yaml` is unredacted (F9).              |
| Start / stop / status       | `reconcile_core`, `stop_core`, `core_status`, `subscribe_core_events`                                                           | Works. `stop_core` has no RPC command; RPC commands drop the `reconcile_core` report. |
| Persistence and recovery    | `profiles.yaml` under injected paths; `startup_reconcile`, `start_background_sources`                                           | Works for orderly restarts. Crash recovery leaves an orphan core (F7).                |
| Shutdown                    | `request_shutdown`, `wait_shutdown`                                                                                             | Works; the core is stopped with the server.                                           |

Rejected changes are `Err` with `CommitAborted` and a `RuntimeAftermath`;
committed changes with degradation are `MutationOutcome::CommittedDegraded`.

### Spike results

x86_64 musl release build in `rust:alpine` 3.24 with Mihomo v1.19.33
(`linux-amd64-compatible`), toolchain `nightly-2026-11-03`:

- Run 1 (fresh): startup `Ready`; the profile and both transforms commit;
  activation applies; Mihomo reaches `Running` and passes its health check. The
  runtime YAML contains the JS marker before the Lua marker. Replacing the JS
  transform with a throwing script is rejected (`RuntimeRefused`,
  `RuntimeAftermath::Untouched`); the file, selection, runtime YAML and core pid
  are unchanged. Stop reaches `Stopped`; shutdown is orderly.
- Run 2 (same directories): profile, transform order and selection are
  retained. `startup_reconcile` does not start the core by itself, whether run 1
  ended with an explicit stop or a crash; `reconcile_core` starts it with the
  persisted chain. Shutdown leaves no core process.
- Crash: after `kill -9` of the server, Mihomo is reparented and keeps running.
  The next server reports `Stopped` and starts a second core (F7).
- Desktop effects (tray, widget, hotkeys, system proxy, auto-launch, locale,
  logger) report `blocked` with a failure code instead of success.
- `pnpm run lint:backend-boundaries` and `lint:architecture-ledger` pass; the
  boundary lint does not list `nyanpasu-server` yet, and its musl dependency
  tree contains no Tauri, wry, egui, Slint, GTK or WebKit crates.

| Measurement                                | Value                        |
| ------------------------------------------ | ---------------------------- |
| `nyanpasu-server` binary, stripped         | 26.8 MB (33.9 MB unstripped) |
| Server peak RSS, JS and Lua transforms     | 23.3 MB                      |
| Server peak RSS, no transforms             | 22.1 MB                      |
| Mihomo RSS                                 | 41 MB                        |
| Client graph construction                  | 0.25–0.5 s                   |
| Release build, cold / server-crate rebuild | 301 s / 127 s (32 cores)     |

The scripts are trivial, so the RSS difference does not bound real transform
cost. aarch64 was not built.

### Findings

| #   | Finding                                                                                                                                                                                                                                                               | Suggested upstream change                                                                             |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| F1  | `ApplicationEffectsPort` plans widget, tray, hotkey and other desktop effects on every host, and `EffectFailureCode` has no generic "unsupported" value; the spike borrows the nearest failure code.                                                                  | A host-declared set of applicable effects, or an explicit unsupported code.                           |
| F2  | `OsProxyPort::default_bypass` returns a string, so "no platform bypass" can only be the empty list.                                                                                                                                                                   | Make "no system proxy" a host capability rather than an adapter answer.                               |
| F3  | A `ServiceHostAdapter::probe` error means "a daemon may hold a core", so startup refuses to start any core (`RecoveryRequired`). The spike reports `NotInstalled`, which is true, but the port has no "this host has no service" answer.                              | Let the host declare that it has no service host.                                                     |
| F4  | `BundleMetadata` is desktop-shaped (window label, WebView, portable layout, updater feeds), so the server invents values.                                                                                                                                             | Split host-neutral build metadata from desktop bundle metadata.                                       |
| F5  | `actor_v2::local_host::build` requires the Windows shutdown helper path on every platform.                                                                                                                                                                            | Make it a Windows-only input.                                                                         |
| F6  | `ServiceHostAdapter` must return an endpoint even when there is no service.                                                                                                                                                                                           | Follows from F3.                                                                                      |
| F7  | A core orphaned by a server crash is neither adopted nor retired; the next start launches a second core with the same listeners. The container had no cgroup v2, so process containment fell back to process groups. Whether the ports conflicted was not determined. | Decide the server-crash policy (below); then persist enough identity to adopt or retire the old core. |
| F8  | `stop_core` has no RPC command; there is no build- or validate-only operation; RPC commands drop the `reconcile_core` report.                                                                                                                                         | Add these when the CLI target is defined.                                                             |
| F9  | `runtime_yaml` returns the controller secret and any credentials unredacted.                                                                                                                                                                                          | Resolve read-only access (below) before exposing it.                                                  |
| F10 | `gxhash` in `nyanpasu-logging` fails to compile without `+aes`, which routers do not guarantee. The spike passes `+aes,+sse2` through `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS` only.                                                                        | Make the hasher portable or feature-gated; do not add AES flags to router builds.                     |
| F11 | Without `/etc/machine-id`, HWID generation logs an error and falls back.                                                                                                                                                                                              | Check OpenWrt images; downgrade the log or inject an identity.                                        |

Other target requirements: `aws-lc-sys`, `zstd-sys` and vendored Lua need a
target C toolchain; rustls reads the system CA store, so the package needs
`ca-bundle`; default path discovery needs `HOME`/XDG, so the server injects
explicit roots.

### Reproduction

From PowerShell, with a long-lived container that keeps the Cargo registry and
target in named volumes:

```text
wslc run -d --name nyanpasu-musl -v F:\Code\clash-nyanpasu:/src:ro -v nyanpasu-target:/cache/target -v nyanpasu-cargo:/usr/local/cargo/registry -v nyanpasu-cargo-git:/usr/local/cargo/git rust:alpine sleep infinity
wslc exec nyanpasu-musl apk add --no-cache build-base cmake git perl pkgconf curl
```

Inside the container (`wslc exec -i nyanpasu-musl sh`):

```sh
cd /src/backend
CARGO_TARGET_DIR=/cache/target \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS='-C target-feature=+aes,+sse2' \
  cargo build --release --locked -p nyanpasu-server --target x86_64-unknown-linux-musl
# Place the Mihomo binary at <data-dir>/mihomo, then:
nyanpasu-server run --config-dir /cache/run/config --data-dir /cache/run/data \
  --rpc-socket /cache/run/rpc.sock
```

The scripted `spike` subcommand that produced the results above was replaced by
`run`: the same loop is driven from outside through the local socket.

## POC plan, 2026-11-05

The user asked for the POC/MVP implementation, a proper RPC configuration, a
WebUI-in-LuCI investigation and a container to try it in. The work stays on
`feat/openwrt-headless-spike`.

### Decisions

Decided by the user on 2026-11-05:

- **Server crash (F7):** the core dies with the server. On Linux the core child
  is bound to its parent's lifetime, and a starting server retires a leftover
  core identified by a persisted pid and process start time before starting its
  own. procd respawns the server.
- **Autostart:** the server restores the last explicit intent. It persists
  start/stop intent and starts the core on startup unless the user last stopped
  it.
- **Read-only YAML (F9):** read-only sessions get a redacted runtime YAML through
  a separate command; the full YAML needs write access. Authorization stays in
  the rpcd ACL, so redaction is a property of the command, not of the caller.

How the first two are implemented in the shared core:

- **Core lifetime:** the core manager already records each core's identity (pid,
  boot-bound process start token and executable) under
  `<config-dir>/runtime/control/` and, when it is built, retires a leftover whose
  identity still matches and never signals a reused pid. The spike saw the second
  core but did not check that the first had been killed, so F7's retirement half
  existed. The missing half was the window between a crash and the restart, now
  closed by `ManagerOptions::kill_core_on_manager_death`
  (`PR_SET_PDEATHSIG` through `nyanpasu_utils::process::Command`). Hosts opt in
  with `CoreLifetime::BoundToHost`; the desktop stays `Independent`.
- **Autostart:** `startup_reconcile` starts the core unless the user last stopped
  it, as the desktop already does within a session. A host that supplies a
  `CoreIntentStore` makes that stop durable: `stop_core` is recorded before the
  core stops, an explicit start before it is tried, and a store that cannot be
  read starts the core. The desktop supplies none.

### RPC: Phase E of the core package split

RPC follows Phase E of the
[core package split](../spec/2026-11-03-core-package-split/design.md) rather
than a router-only transport:

| Step     | Scope in this POC                                                                                                                                                                             |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1a      | Neutral macro declarations, `rpc(local)` opt-in and capability metadata; command names, event names and the `RpcError` envelope in `nyanpasu-rpc-protocol`. Bindings regenerate without diff. |
| E2       | `nyanpasu-rpc`: neutral registry and event bus; Tauri events, SSE and the local socket subscribe to it. Tauri-free commands needed by the MVP move first.                                     |
| E3       | API-only HTTP mode (assets optional) and the local-socket transport, both enforcing the registry's allowlists.                                                                                |
| E4 (POC) | `nyanpasu-server run` and `nyanpasu-cli`. Deviation: C3 and E1b have not landed, so the CLI passes JSON without typed DTOs and the server still links desktop-shaped core code.               |

### Local interface

- **Socket:** `/var/run/nyanpasu/rpc.sock`, directory `0700` and socket `0600`
  owned by root; the server takes `--rpc-socket PATH`. Socket permissions are
  the only transport authentication; rpcd ACLs authorize LuCI sessions.
- **Frames:** a big-endian `u32` length followed by UTF-8 JSON, at most 4 MiB.
  The client sends `{"v":1,"id":N,"method":"...","params":{...}}`; the server
  replies `{"v":1,"id":N,"result":...}` or `{"v":1,"id":N,"error":RpcError}`. A
  connection may carry several requests; disconnecting does not cancel work
  the server has started.
- **CLI:** `nyanpasu-cli [--socket PATH] [--timeout SECS] call METHOD [--params JSON | --params-file FILE]`
  prints the result or `RpcError` as JSON on stdout. Exit codes: 0 success, 1
  `RpcError`, 2 transport failure, which prints
  `{"kind":"transport_error","message":...,"outcome_unknown":bool}` and, for a
  deadline (default 90 s, as a reconcile can take about 30 s) or a closed
  connection, reports the outcome as unknown because the server keeps running
  work its caller left. A usage error exits 2 with a message on stderr.
  `nyanpasu-cli methods` lists the `local` allowlist through `list_methods`.
  Parameters are the command's arguments as one JSON object whose top-level keys
  are camelCase (`fileData`, not `file_data`), while the nested DTOs keep their
  own snake_case; an unknown key is ignored, so a misspelled `fileData` creates
  an empty profile.
- **`rpc(local)` allowlist and rpcd ACL:**

| ACL   | Commands                                                                                                                                                                                                                                   |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| read  | `get_core_status`, `get_profiles`, `get_runtime_yaml_redacted` (new), `inspect_runtime`, `query_core_logs`, `get_core_log_status`, `list_methods` (new)                                                                                    |
| write | `create_profile`, `delete_profile`, `replace_profile_definition`, `set_global_transforms`, `activate_profile`, `read_profile_file`, `save_profile_file`, `reconcile_core` (new, returns the report), `stop_core` (new), `get_runtime_yaml` |

`read_profile_file` is write-only because configuration profiles carry
credentials. `update_core` and other commands that can reach elevation stay out
of the allowlist.

### LuCI and packaging

- **rpcd:** a ucode plugin publishes the ubus object `nyanpasu`, one method per
  allowlisted command, each running `nyanpasu-cli call`. Parameters go through
  a temporary file, never through a shell-interpolated argument.
- **ACL:** `luci-app-nyanpasu` grants the read methods to `read` and the write
  methods to `write`, with no wildcards.
- **Views:** native LuCI JavaScript under Services → Nyanpasu: Overview (status,
  start/stop, selected profile, redacted YAML), Profiles (import YAML, create
  JS/Lua transforms, order, activate) and Logs.
- **Service:** a procd init script at `/etc/init.d/nyanpasu` with respawn; config
  in `/etc/nyanpasu`, data in `/usr/share/nyanpasu` or the overlay, and the core
  at `<data-dir>/mihomo`.
- **Try-out container:** `openwrt/rootfs:x86-64-25.12.6`, which uses apk, run
  with `/sbin/init` and LuCI published on `localhost:8080`. The default
  capability set has no `CAP_NET_ADMIN`, so network and firewall pages do not
  work. Files are installed with `wslc container cp` until feed packages exist.

### WebUI in LuCI

The React app already runs in a browser: outside Tauri it calls `POST
/bridge/rpc` and listens on `GET /bridge/events` (SSE). It can be offered from
LuCI once E3 lands, with these gaps:

- **Server:** the HTTP host is loopback-only, rejects other `Host` values and
  refuses to start without assets. Events are bridged from Tauri.
- **Auth:** the per-start token suits a desktop. A router needs a LuCI-issued
  one-time ticket (a write-ACL command) that is exchanged for the session cookie.
- **Size:** the bundle is 24 MB, with Monaco about 13 MB, and expects to be
  served at `/`. A router route set without Monaco, tray, widget and service
  pages would cut most of it. Profile editing opened a Tauri-only editor window,
  which now opens as a browser tab outside Tauri.

Order: native LuCI views first. The WebUI follows E3 as a link from LuCI to the
server's HTTP port with a ticket, then as a reduced router build.

### Units

1. E1a–E3 in shared crates, keeping the desktop build and bindings unchanged.
2. Core lifecycle for F7 and autostart in the shared core manager and startup.
3. `nyanpasu-server run` and `nyanpasu-cli` over the local socket.
4. `luci-app-nyanpasu` (rpcd plugin, ACL, menu, views) and the procd init script.
5. Container integration: install, LuCI walkthrough, recorded results.
6. WebUI link with ticket auth, if E3 allows it without the reduced build
   (done, see [WebUI link](#webui-link-2026-11-05)).

Still open: F1 and F3–F6 (the upstream C1/C2 host capabilities), F10
(`gxhash`), aarch64, and realistic transform measurements.

## Container integration, 2026-11-05

Unit 5. The real `nyanpasu-server`, `nyanpasu-cli` (stripped musl builds) and
Mihomo ran in an `openwrt/rootfs:x86-64-25.12.6` container under procd, with
LuCI on host port 8080. Mihomo sits at `/usr/share/nyanpasu/mihomo`, the
server's `<data-dir>/mihomo`. The container has no `CAP_NET_ADMIN`, so the
network and firewall pages fail as expected; nothing in the MVP needs them.

### What works

- The MVP loop through LuCI over HTTP (`/ubus` login, then the three views in a
  headless browser): import a YAML profile, create a JavaScript transform,
  attach, reorder and detach it, select the profile, Start / apply, Stop, read
  the redacted and the full runtime YAML, read the logs.
- Failures show as LuCI notifications with the RpcError kind, the domain kind
  and the domain detail. An unreachable server shows its own message and
  whether the command may still have run.
- State persists across a server restart: the selected profile, the profile
  list and the last start or stop intent.

### Verified

| Check                                  | Root                                     | `viewer` (read ACL only)                       |
| -------------------------------------- | ---------------------------------------- | ---------------------------------------------- |
| The six read methods                   | allowed                                  | allowed                                        |
| The ten write methods, incl. full YAML | allowed                                  | `Access denied`, each one                      |
| Redacted YAML                          | `secret: <redacted>`                     | `secret: <redacted>`                           |
| Full YAML                              | the real secret, behind a warning        | denied; the checkbox is not rendered           |
| Views                                  | no JS error beyond LuCI's own login page | read-only banner; no buttons, forms or deletes |
| `session.access` probe for `stop_core` | true                                     | false                                          |

### Crash policy under procd

The init script respawns after 5 s and gives up after 5 failures that each
happen within 3600 s of a start.

| Case                                                                | Result                                                                     |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `kill -9` the server with the core running                          | procd respawns it; the old Mihomo is gone within a second, never two cores |
| The same, with the last intent `stop_core`                          | the core stays stopped                                                     |
| Two further kills                                                   | respawned each time, core still stopped                                    |
| `/etc/init.d/nyanpasu restart` after `stop_core` / `reconcile_core` | stopped stays stopped; running comes back running                          |

The orphaned core of F7 did not survive here. Why Mihomo died with the server
is not established (procd tearing the instance down, or the process group);
the same crash outside procd was not tried. Cgroup containment was still
unavailable (`failed adding instance cgroup`).

### Findings

| #   | Finding                                                                                                                                                                                                                                                                                                                 | Direction                                                                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| F12 | A profile whose YAML root is not a mapping (empty, comment-only, a bare scalar) is accepted by `create_profile` and `activate_profile`, then panics `get_runtime_yaml_redacted` and `inspect_runtime` in core (`runtime_inspection.rs:304`, root `Null`; a scalar root panics in the graph restore). Not fixed in core. | Validate the root at profile commit, or return an error from inspection, not a panic. |
| F13 | Before the transport caught handler panics, F12 left the CLI waiting for its deadline and then reporting an unknown outcome. `serve_local` now answers a panicking command with an `internal` RpcError and keeps the connection.                                                                                        | Done for the local socket and for HTTP dispatch.                                      |
| F14 | Parameter keys are camelCase (`fileData`, not `file_data`) and unknown keys are ignored silently, so a misspelled optional key is a no-op.                                                                                                                                                                              | Reject unknown keys in the command DTOs.                                              |
| F15 | The status is a projection: it lags `stop_core` by about two seconds, can be empty right after a reconcile, and the status inside a mutation reply can be stale. LuCI re-reads it until the expected state shows.                                                                                                       | Keep this as the contract, or have lifecycle replies carry the settled state.         |
| F16 | The headless server never started the Clash streams, so the core log store stayed empty. It now starts them as the desktop shell does. The resident cost on a router was not measured.                                                                                                                                  | Measure memory with the streams running on a real router.                             |
| F17 | `reconcile_core` returns the effective config, including the controller secret, in `effective_config`. It is under the write ACL, so only root gets it, but a caller that logs replies would leak it.                                                                                                                   | Drop the config from the local reply, or redact it.                                   |
| F18 | procd's `respawn 3600 5 5` ends the respawning after five crashes within an hour; only a `restart` resets it.                                                                                                                                                                                                           | Decide whether a router should give up, and how it reports that.                      |

LuCI has no profile editor, so there is no save path to guard. Import refuses
an empty or non-mapping YAML in the browser (F12); nothing stops another ubus
caller with the write ACL from sending one.

### Trying it

Superseded by the [WebUI link](#webui-link-2026-11-05): the native views below
were replaced by the WebUI and a minimal service page. The container steps are
kept as the record of what was verified.

The container `nyanpasu-openwrt` is left running with the server enabled and
started, the profile `Sample` (with the transform `Sample tweak`) selected and
the core running.

1. Open <http://localhost:8080/> and log in as `root` / `nyanpasu`. The login
   `viewer` / `viewer` has only the read ACL.
2. Services → Nyanpasu → Overview: the core is Running. Click Stop, then
   Start / apply; the status settles within a few seconds. Tick the checkbox
   under Runtime configuration to see the full YAML.
3. Profiles: import a YAML (a name and text, or a file), select it with
   Activate, then Start / apply on the Overview. Create a script transform and
   attach it under Transforms of the selected profile.
4. Logs: Mihomo log lines appear once traffic passes the core, for example
   `http_proxy=http://127.0.0.1:7890 wget -qO- http://127.0.0.1/` inside the
   container.

To start over: `wslc exec nyanpasu-openwrt /etc/init.d/nyanpasu restart`.

## WebUI link, 2026-11-05

Unit 6, then a change of direction.

### Decision

The user decided on 2026-11-05 that the React WebUI is the management UI and
replaces the native LuCI views, which were deleted with `api.js`. That day the
WebUI was first embedded in LuCI in a frame; the user then reversed this and
stopped embedding it. Services → Nyanpasu is a minimal native page and the WebUI
opens in a new window. LuCI implements neither the management UI nor its API,
and the WebUI talks to `nyanpasu-server`'s HTTP API directly. This reverses the
MVP's "do not embed its React distribution" and the native-views product
outcome only in part ([MVP](openwrt-mvp.md#decision-the-webui-is-the-main-ui-2026-11-05)).

The page has only the most basic operations:

- the service status, from procd through the `rc` ubus object (`rc list`);
- Start, Stop, Restart and a Start at boot toggle (`rc init`), LuCI's standard
  facility with its standard ACL grants, as luci-mod-system uses it; there is
  no management API of our own;
- Open WebUI, which opens the window synchronously in the click handler (so the
  popup blocker allows it), clears its `opener`, and sets its location when
  `webui_ticket` returns the ticket; if the ticket fails the window is closed
  and the reason shown in LuCI;
- a selector for how that window opens, stored in the UCI option
  `nyanpasu.main.window_mode`: `tab` (default) or `popup`, a simplified window
  from `window.open` features (`popup,width=1280,height=800`). It is LuCI's
  setting only: the page stages it with `uci set` and commits that one config
  (`uci commit`, in the write ACL), without restarting the service; the pid
  was unchanged in the container. `reload_config` is not run, so a later Save &
  Apply elsewhere in LuCI may see the `nyanpasu` config as changed and reload the
  service once (procd's reload trigger compares config checksums; not
  exercised). The WebUI is not told about it.

Consequences:

- **The F9 read-only role is back, as a status page.** A login with the `read`
  ACL (`rc list`, UCI) sees the status and no buttons; the WebUI still has no
  read-only mode, and only a login that may run `webui_ticket` can open it.
  `get_runtime_yaml_redacted` stays on the local socket, unused by LuCI.
- **The 16-method LuCI allowlist became the HTTP command set**, a wider
  surface (see the security notes). rpcd has one method, `webui_ticket`.
- **The WebUI's editor windows open as browser tabs.** Outside Tauri the app
  opens `/editor/profile?uid=...` and `/editor/css` with `window.open(url,
'_blank')` instead of the desktop-only `create_editor_window` command.
- **The bundle** is 24,053,560 bytes in 428 files (22.9 MiB; about 5.3 MiB
  gzipped for JS, CSS and HTML), with Monaco and its workers about 12 MiB of
  it. The stripped musl server is 28.0 MB with the HTTP listener.

### Design

```text
LuCI page -> rpcd webui_ticket (write ACL) -> nyanpasu-cli -> local socket
          -> issue_web_ticket -> { "path": "/?ticket=...", "port": 7899 }
new window location = location.protocol//location.hostname:7899 + path
GET /?ticket=T  -> consume T, Set-Cookie session, 303 /
cookie          -> POST /bridge/rpc, GET /bridge/events, static assets
```

- **Listener:** `nyanpasu_rpc::http::RouterListener` sits beside the desktop's
  `HttpListener`, which is unchanged. It binds the given address, owns the
  ticket and session book and returns the issuing half, which the composition
  root injects into the registry as `WebTickets` for `issue_web_ticket`.
- **Tickets:** 128 random bits, single use, valid 60 s, at most 16 outstanding.
  A wrong or late attempt opens nothing and a wrong guess does not consume a
  real ticket.
- **Sessions:** a random UUID cookie, `HttpOnly; SameSite=Strict; Path=/`, kept
  in memory, at most 8 (the oldest is evicted) and lost on restart. They have
  no idle timeout and there is no logout.
- **Host and Origin:** no fixed authority, so any `Host` is served; a request
  that carries `Origin` must carry `http://{Host}`. The cookie is the
  credential, so DNS rebinding has nothing to ride on.
- **Framing:** every response carries `X-Frame-Options: DENY` and
  `Content-Security-Policy: frame-ancestors 'none'`. Nothing frames the WebUI;
  LuCI opens it as a top-level window, so `SameSite=Strict` applies to its own
  requests.
- **Server:** `nyanpasu-server run --http-listen ADDR:PORT [--webui-dir DIR]`.
  Without `--http-listen` there is no listener; without a directory it serves
  the API only. Assets use the Tauri crate's asset router, moved into
  `nyanpasu-rpc` (`FrontendAssets`, `assets_router`) with a directory source in
  the server.
- **OpenWrt:** `/etc/config/nyanpasu` (`webui_listen '0.0.0.0:7899'`,
  `webui_dir '/usr/share/nyanpasu/webui'`) is read by `nyanpasu.init`; an empty
  `webui_listen` turns the WebUI off. The default is on, now that it is the main
  UI. The init file is still mode 100755.
- **HTTP panics:** HTTP dispatch now turns a handler panic into an `internal`
  RpcError, as the local socket does (folded into the transport commit).

### Security notes

- **A session is a full user of the HTTP command set**, which is not the
  local allowlist. Over HTTP: `get_profiles`, `create_profile`,
  `delete_profile`, `activate_profile`, `set_global_transforms`,
  `replace_profile_definition`, `read_profile_file`, `save_profile_file`,
  `get_runtime_yaml` (the full YAML, secrets included), `inspect_runtime`,
  `get_core_status`, `query_core_logs`, `get_core_log_status`. Not over HTTP:
  `reconcile_core`, `stop_core`, `get_runtime_yaml_redacted`, `list_methods`
  and `issue_web_ticket` (a session cannot mint tickets). Write access to
  profiles is code execution in the server, since profiles carry JS/Lua
  transforms, and the server runs as root. Whoever can run `webui_ticket` is
  therefore root-equivalent, as with the old write ACL.
- **The set will grow** as the Tauri-only commands the WebUI calls (app config,
  storage, Clash API proxies, system proxy) move into `nyanpasu-rpc`; each needs
  the review `rpc(http)` already requires. It did, see the
  [WebUI RPC and events](#webui-rpc-and-events-2026-11-05) security note.
- **LAN exposure:** the port listens on `0.0.0.0` in plain HTTP and is open on
  the LAN, relying on the firewall's WAN input reject. There is no TLS, so a
  ticket (valid 60 s) and the session cookie can be read by anyone on the path.
- **Cookies are not port-scoped:** the browser sends LuCI's session cookie to
  port 7899 and ours to LuCI. Both are `HttpOnly`; the servers ignore each
  other's cookie.
- **Brute force:** a ticket is 128 random bits, and a failed attempt changes
  nothing, so guessing is not a practical route.

### Verified

On the try-out container (the stripped musl binaries and the built WebUI):

| Check                                             | Result                                                                                                                                                                                                                            |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `webui_ticket` over `/ubus`, root                 | `{ok:true, result:{path:"/?ticket=...", port:7899}}`                                                                                                                                                                              |
| The same, `viewer` (read ACL)                     | `Access denied`; the `session.access` probe is false                                                                                                                                                                              |
| `curl`: ticket URL                                | 303 to `/`, cookie `HttpOnly; SameSite=Strict`, `no-store`, `frame-ancestors` for Host                                                                                                                                            |
| `curl`: ticket reuse, no cookie, foreign `Origin` | 401, 401, 403                                                                                                                                                                                                                     |
| `curl`: cookie                                    | `POST /bridge/rpc get_profiles` 200 with the Sample profile; `GET /` the app                                                                                                                                                      |
| A session minting a ticket                        | 501 `unsupported`                                                                                                                                                                                                                 |
| LuCI as root, Services → Nyanpasu                 | status Running; Stop, Start and Restart observed with `/etc/init.d/nyanpasu status` and `pgrep` (no server or core after Stop; both back after Start; new server pid after Restart); Start at boot toggles `enabled`              |
| Open WebUI, New tab                               | `window.open('about:blank', '_blank')` in the click, then the ticket URL; no `opener`; the new tab is signed in (`get_profiles` 200)                                                                                              |
| Open WebUI, Popup window                          | `window.open` with `popup,width=1280,height=800`; the window reports 1280 x 800 and no toolbar, menubar or location bar; signed in                                                                                                |
| `window_mode` setting                             | `uci get` shows the new value, no uncommitted changes, the server pid is unchanged                                                                                                                                                |
| Open WebUI with the service stopped               | the new window is closed and LuCI shows the socket error                                                                                                                                                                          |
| WebUI profile editor                              | the profile card's edit button opens `/editor/profile?uid=...` in another tab (`window.open(url, '_blank')`) with the file loaded; an edit and Save closed the tab and `read_profile_file` returned it; the original was restored |
| LuCI as `viewer` (read ACL)                       | the status and the boot setting as text; no buttons, no inputs                                                                                                                                                                    |
| Core and CLI                                      | core running after the restart; `reconcile_core` and `get_core_status` work                                                                                                                                                       |

Unit tests cover the ticket book (single use, expiry, bad ticket, the caps), the
listener (missing cookie 401, Origin mismatch, any Host, no framing), the
HTTP panic, the server flags and, on Linux, the whole flow from a socket ticket
to a session.

### Which WebUI pages work

Superseded by [WebUI RPC and events](#webui-rpc-and-events-2026-11-05), which
moved these commands. The record of the first result follows.

The server links only the commands in `nyanpasu-rpc`. Everything else the app
calls lives in the Tauri crate and answers `unknown_method` (404). The app shell
renders; nothing was fixed in the frontend.

| Page                                                                    | Result                                                                                                                                                                                                                                           |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Profiles                                                                | Works: the Sample profile, its transform and `read_profile_file` load. Traffic columns fail (`query_traffic_report`).                                                                                                                            |
| Logs                                                                    | The core log query works (`query_core_logs`); it shows what the store holds.                                                                                                                                                                     |
| Dashboard, Proxies, Rules, Providers, Connections, Traffic and Settings | Empty or "Could not load configuration": `get_app_config`, `get_proxies`, `get_storage_item`, `get_clash_config`, `clash_api_*`, `get_sys_proxy`, `query_traffic_report`, `get_traffic_summary` and `report_frontend_events` are not registered. |

Profile editing opens the editor as a browser tab (see the verified table).
Creating and deleting through the UI were not exercised. The CSS editor route
loads, but saving its CSS needs `get_storage_item` and `set_storage_item`, and
Ctrl+click on a link in the profile editor calls `open_that`; none is registered
in the server.

### Remaining work for the reduced build

- ~~Move the commands the WebUI needs (app config, storage, proxies and the Clash
  API, traffic) into `nyanpasu-rpc` with a review of their HTTP exposure~~ (done,
  see [WebUI RPC and events](#webui-rpc-and-events-2026-11-05)).
- Build a router route set without Monaco, tray, widget and service pages, and
  serve it precompressed.
- Package the WebUI (a `nyanpasu-webui` package at `webui_dir`).
- A read-only WebUI mode, session expiry and logout.
- TLS, or a path through uhttpd; consider listening on the LAN address instead
  of `0.0.0.0`.
- Mutations through the WebUI on a router; the resident memory of the server
  with the listener on real hardware.

## WebUI RPC and events, 2026-11-05

The [WebUI link](#webui-link-2026-11-05) served the React app, but only the
commands in `nyanpasu-rpc` existed on the server. The app shell asks for
`get_app_config` before it renders a page, so a browser got a blank page on
every route. This section records the fix: the commands the pages need moved
into `nyanpasu-rpc`, and the events the pages refresh from now reach the
browser. The inventory behind it (every command and event the frontend uses,
with its class) was taken first and is the basis of the table below.

### What moved

72 commands moved from `backend/tauri/src/ipc.rs` to modules under
`backend/nyanpasu-rpc/src/commands/`, by domain, with their doc comments and
signatures, so the desktop's generated bindings are unchanged. `get_core_version`
also opted into `rpc(http)`. The HTTP command set grew from 13 to 85 and is
pinned by a test (`the_http_allowlist_is_the_reviewed_webui_command_set`).

| Domain            | Commands                                                                                                                                     |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| App configuration | get/patch app and Clash config, runtime overrides, config features, Clash info, configuration status and its two retries                     |
| Storage           | the five `web:` key-value commands, so the CSS editor saves                                                                                  |
| Host              | `is_portable`, `status_service`, `probe_direct_egress`, `get_cached_icon`                                                                    |
| Clash             | proxies, `select_proxy`, rules, providers, delay tests, connection close, `get_sys_proxy`                                                    |
| Core manager      | `change_clash_core`, `restart_sidecar`, versions, updater inspection, agent skills                                                           |
| Traffic           | the Clash websocket snapshot and recording, and the six `query_traffic_*` reads                                                              |
| Logs              | `get_core_log`, `clear_core_logs`, log files and sessions, `report_frontend_events`                                                          |
| Profiles          | import, ordering, layout, folders, sync status, `update_profile`, metadata and remote-option patches, the applied runtime and its node views |

Two rules let them move without a Tauri type:

- **`caller: Caller`** is the neutral counterpart of the desktop `Window` for
  `rpc(owner)` commands. A browser session owns the log sessions it opens, and
  another session cannot close them; on the desktop the owner is still the
  window label.
- **`rpc(optional_capabilities(...))`** records the effects a command only may
  reach. `patch_app_config` and `retry_configuration_effect` may touch the
  system proxy, hotkeys and the tray. As required capabilities they would keep
  the headless server from registering them, and saving any setting would have
  failed. The effects port already reports each effect it cannot apply as
  unsupported, never as applied, so the commands are served and say so.

The traffic store opener moved from the desktop setup into core
(`TrafficFileStorage::open_in_data_dir`), and the server now opens it, so it
records traffic like the desktop. The history sits under the data directory,
which is flash on a router, and the client sizes its cache for a desktop. Where
that history should live on a device is not decided.

### Events

Seven shared sources were forwarded to the event bus by tasks in the Tauri
setup; the server forwarded none. `nyanpasu_rpc::forward::forwarders`
subscribes to all of them, and each shell runs the returned futures under its
own shutdown owner: the desktop through `tauri::async_runtime`, the server
through `tokio`. `/bridge/events` already delivered every bus event to a
router session, so it needed no change. Events the browser receives:

| Event                                                                                                  | Source                                           |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------ |
| `state-changed-event`                                                                                  | state commits and proxy changes                  |
| `core-status-changed-event`, `service-status-changed-event`                                            | core and service owners                          |
| `configuration-status-changed`                                                                         | mutation journal, effects and profile sources    |
| `clash-ws-event`                                                                                       | the Clash streams (traffic, memory, connections) |
| `core-logs-changed`                                                                                    | the core log store                               |
| `storage-value-changed-event`, `storage-resync-event`                                                  | the key-value storage                            |
| not on the server: `app-update-state-changed`, `scheme-request-received-event`, `window-message-event` | the desktop updater, deep links and windows      |

The desktop's event types stay as the description of the payloads in the
generated bindings, and a test keeps their wire shape equal to what the shared
publishers send.

### Pages

Before, every route rendered a blank page (`get_app_config` and
`get_clash_ws_snapshot` answered 404 `unknown_method`). After, on the container
with the real server and Mihomo, as root through LuCI → Open WebUI. A page's
commands are the requests it made in its first five seconds; "Work" means no
request failed except as listed.

| Page                                                       | After                                                                                                                                                                               |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dashboard                                                  | Works; 11 commands, live traffic, memory and core status. `get_sys_proxy` answers 500 (no OS proxy on this host), the proxy tile shows Disabled.                                    |
| Proxies, Rules, Providers                                  | Work; the groups, rules and providers of the selected profile load.                                                                                                                 |
| Connections                                                | Works; the active list went 0 to 3 while three proxy tunnels were held open, and the closed list grew when they ended.                                                              |
| Traffic                                                    | Works; the store records (3 active connections in `get_traffic_summary` while tunnels were held open).                                                                              |
| Logs                                                       | Works; core log lines arrive live.                                                                                                                                                  |
| Profiles                                                   | Works; the count changed from 1 to 2 when a profile was created through the CLI, without a reload, and back on deletion.                                                            |
| Settings: Clash (core, control, network, runtime, AI)      | Work.                                                                                                                                                                               |
| Settings: Clash core manager                               | Works; Mihomo's version shows, the cores that are not installed say "Could not read core version" (`get_core_version` 500, an honest answer).                                       |
| Settings: Nyanpasu interface, logs, widget, updates, About | Render; `get_system_accent_color` is 404 on the interface page, which shows the configured colour. The CSS editor's `set_storage_item` and `get_storage_item` round-trip over HTTP. |
| Settings: System proxy                                     | Renders; `get_sys_proxy` answers 500.                                                                                                                                               |
| Settings: Debug                                            | Renders.                                                                                                                                                                            |

Live checks: the browser's own `EventSource` saw `clash-ws-event`,
`core-logs-changed` and `core-status-changed-event` while traffic passed the
core; a profile created behind the Profiles page's back through the CLI changed
its count without a reload, and so did its deletion; the dashboard's totals
counted the traffic.

### Desktop-only remainder

These stay in Tauri. The pages above still render around them:

- **Keyboard shortcuts** page: `get_hotkeys`, `get_hotkey_functions` (404). The
  page shows an empty card.
- **Tray** page: `get_tray_icon` and the tray and widget icon commands (404).
- **System service** page: `get_service_install_prompt`, `get_core_dir`, the
  service, DNS and UWP commands. `status_service` is served and reports "Not
  installed".
- **Interface** page: `get_system_accent_color` (404).
- **Updates and About**: the app updater and release channel
  (`capabilities(app_update)`); the server has no updater backend.
- **Elsewhere**: `update_core` (elevation fallback), `probe_profile_url`
  (reviewed as desktop-only because it requests any URL the caller names),
  dialogs, clipboard, opener, window and lifecycle commands, and the
  connection-detail Channel (browsers use `/bridge/connection-details`).

Ten commands that no page calls (`enhance_profiles`, `reorder_profile`,
`get_runtime_config`, `get_runtime_exists`, `get_postprocessing_output` and
others) were left where they are so the HTTP set grows only by what a page
uses.

### Security note

The HTTP command set grew from 13 to 85. A browser session, and so whoever may
run `webui_ticket`, can now also read and change the application and Clash
configuration (including the controller secret and ports), close connections,
change the selected proxy, restart the core, switch the core binary, read the
recorded traffic history (browsing targets), read the core and application
logs, and import profiles from arbitrary URLs (`import_profile` fetches the URL
the caller names). Profile import also feeds the JS/Lua transforms, which run in
the server as root, as before. No command that can reach elevation, the system
service, the clipboard, dialogs or the opener is registered on the server. The
auth model is unchanged.

### Verified

Unit tests cover the pinned HTTP set, caller-owned log sessions, the storage
namespace, the forwarders (storage and a reconcile reach the bus) and the wire
shape of the forwarded payloads. On the musl target, `nyanpasu-server`'s test
signs a browser in through a ticket and receives `storage-value-changed-event`,
`state-changed-event` and `configuration-status-changed` over `/bridge/events`.

## Decisions deliberately deferred

Server-crash policy, autostart and read-only YAML access were decided on
2026-11-05 (above). Still to finalize: script capability/resource limits,
payload limits, SDK/release targets and profile-to-runtime apply semantics,
using the upstream APIs and target evidence.

The first product is the server with the React WebUI linked from LuCI (the user's
2026-11-05 decisions, below) for small local profiles, ordered JS/Lua
transforms and Mihomo lifecycle. Transparent proxy, firewall/DNS/TUN takeover,
remote subscriptions, updater, traffic history and the reduced router WebUI
build remain later work. No router networking changes are part
of the paused MVP.

## OpenWrt-nikki reference track

User-selected reference: [OpenWrt-nikki](https://github.com/nikkinikki-org/OpenWrt-nikki),
reviewed at `7b203f6c4c5e94c6c0026acb301090aa1d310e7f` on 2026-10-08.
Refresh this reference alongside main when resuming. Reference review during pause
is design work, not authorization to build, install or deploy either project.

Use these sources to plan host integration after upstream completion:

- [Backend package](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/nikki/Makefile): package installation, conffiles and upgrade retention. Nikki separates backend, LuCI and Mihomo packages; evaluate the same separation for our feed, with dependencies limited to actual MVP capabilities.
- [procd init](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/nikki/files/nikki.init): instance setup, config-file tracking, optional reload signal, respawn and process resource controls. Our shared application must retain config/lifecycle decisions; do not copy init-script orchestration as a second workflow.
- [rpcd ucode](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/luci-app-nikki/root/usr/share/rpcd/ucode/luci.nikki) and [ACL](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/luci-app-nikki/root/usr/share/rpcd/acl.d/luci-app-nikki.json): native LuCI integration reference. Its ucode RPC and wildcard/file permissions are not our CLI and server contract; retain explicit method permissions and shared facade calls.
- [SDK workflow](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/.github/workflows/build-packages.yml): release/architecture matrix and separate package artifacts. Start with the agreed two targets, and select opkg/apk packaging by the chosen release rather than assuming one package manager universally.

Nikki's networking/transparent-proxy integration is a later reference track,
not an MVP dependency. Do not adopt its firewall/kernel-module dependencies,
UCI profile processing, routing takeover or generic core API forwarding merely
because they are present. Preserve shared profile/script/transaction ownership.
Before any future code reuse, check its GPL license and source attribution;
this reassessment only records design references, with no copied source code.
