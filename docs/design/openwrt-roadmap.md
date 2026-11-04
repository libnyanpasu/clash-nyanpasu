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
nyanpasu-server spike --config-dir /cache/run/config --data-dir /cache/run/data --fresh
nyanpasu-server spike --config-dir /cache/run/config --data-dir /cache/run/data
```

`--hold` keeps run 1 alive after the core starts, for crash tests;
`--no-scripts` skips the transforms.

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
- **CLI:** `nyanpasu-cli [--socket PATH] call METHOD [--params JSON | --params-file FILE]`
  prints the result or `RpcError` as JSON on stdout. Exit codes: 0 success, 1
  `RpcError`, 2 transport failure. `nyanpasu-cli methods` lists the `local`
  allowlist.
- **`rpc(local)` allowlist and rpcd ACL:**

| ACL   | Commands                                                                                                                                                                                                                                   |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| read  | `get_core_status`, `get_profiles`, `get_runtime_yaml_redacted` (new), `inspect_runtime`, `query_core_logs`, `get_core_log_status`                                                                                                          |
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
  pages would cut most of it. Profile editing opens a Tauri-only editor window.

Order: native LuCI views first. The WebUI follows E3 as a link or iframe from
LuCI to the server's HTTP port with a ticket, then as a reduced router build.

### Units

1. E1a–E3 in shared crates, keeping the desktop build and bindings unchanged.
2. Core lifecycle for F7 and autostart in the shared core manager and startup.
3. `nyanpasu-server run` and `nyanpasu-cli` over the local socket.
4. `luci-app-nyanpasu` (rpcd plugin, ACL, menu, views) and the procd init script.
5. Container integration: install, LuCI walkthrough, recorded results.
6. WebUI link with ticket auth, if E3 allows it without the reduced build.

Still open: F1 and F3–F6 (the upstream C1/C2 host capabilities), F10
(`gxhash`), aarch64, and realistic transform measurements.

## Decisions deliberately deferred

Server-crash policy, autostart and read-only YAML access were decided on
2026-11-05 (above). Still to finalize: script capability/resource limits,
payload limits, SDK/release targets and profile-to-runtime apply semantics,
using the upstream APIs and target evidence.

The first product remains a server driven through the CLI by native LuCI for small local
profiles, ordered JS/Lua transforms and Mihomo lifecycle. Transparent proxy,
firewall/DNS/TUN takeover, remote subscriptions, updater, traffic history and
full React embedding remain later work. No router networking changes are part
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
