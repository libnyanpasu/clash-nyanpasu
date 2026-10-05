# OpenWrt / ImmortalWrt MVP

Status: downstream design, 2026-10-06. First implement the
[application/platform extraction](application-platform-extraction.md), then
compose the shared application in the router host. This is not a proposal to run
the desktop executable on a router.
Current development standards in `docs/development/` remain authoritative.

## Agreed scope

The first target is aarch64 and x86_64 Linux with musl. Develop in the current
checkout and verify locally with Docker. Prefer ImmortalWrt's official rootfs;
fall back to OpenWrt's official rootfs if the matching image is unavailable.
There is no physical router available for this iteration.

The usable loop is:

1. Log into LuCI and open Services → Nyanpasu.
2. Import a small local YAML profile, optionally with ordered JavaScript/Lua
   transforms, and choose the selected profile.
3. Build and inspect the resulting Mihomo configuration using the same runtime
   pipeline and script engines as the desktop application.
4. Ask the daemon to validate and publish that configuration, start Mihomo
   through procd, inspect actual process state, and stop it.
5. Restart the daemon and verify the selected profile and scripts survive.

Transparent proxying, nftables/firewall4 rules, DNS interception, TUN routing,
subscription scheduling, proxy selection, traffic history, updater, and complete
React UI reuse are subsequent milestones. They are not silently enabled by this
MVP. The initial core listener is the loopback mixed proxy on port 7890.

The native transport is ubus. No OpenRPC or JSON-RPC server is introduced in the
daemon. LuCI already uses rpcd's authenticated ubus access; its existing browser
protocol does not require an additional Nyanpasu protocol layer.

## Existing code and extraction boundary

`backend/nyanpasu-config` already contains profile models, reference validation,
runtime graph execution, merge semantics, and the `ProfileContentSource` /
`ScriptRunner` ports. The higher-level `RuntimeBuilder`, built-in transforms and
Boa/Lua adapters currently live in `backend/tauri/src/enhance`. Their domain
behavior can be shared without moving desktop windows or lifecycle code.

There are two important obstacles:

- `nyanpasu-config` imports `StatisticWidgetVariant` from `nyanpasu-egui`, which
  pulls eframe and platform GUI dependencies into an otherwise reusable model.
- The desktop `NyanpasuClient`, state actors, transactional profile
  materialization, runtime workflow and UnifiedRpc are still housed inside the
  Tauri crate. Moving that whole graph in one patch would also move logging,
  hotkeys, tray/window effects, storage, updater and desktop service control.

The first extraction moves the runtime builder and built-in sources into
`backend/nyanpasu-application`, and concrete script engines/filesystem adapters into
`backend/nyanpasu-platform`. Desktop callers use those shared implementations.
The widget wire enum moves to
`nyanpasu-helper`; config and egui both consume it, preserving its serialization
and clap behavior while removing config's dependency on GUI code.

Desktop-specific runtime projection and filesystem snapshot adapters stay in
Tauri. `ScriptDirs` receives ordinary paths; the desktop composition root builds
them from its existing path resolver. The shared crate never imports Tauri.

The desktop facade and transactional profile workflows must be extracted by
complete vertical slices before the router composes those use cases. A second
router facade/store/journal is not the migration solution. The sections below
describe required router behavior; their ownership and final DTOs are reconciled
with the shared application API as that extraction proceeds.

## Packages and dependency direction

```text
Tauri composition root                  OpenWrt composition root
  desktop NyanpasuClient                  router NyanpasuClient
  desktop persistence/effects             StateActor + typed facade
             \                           /
              nyanpasu-application
              shared clients + RuntimeBuilder
                        |
              nyanpasu-platform (injected by host)
              Boa/Lua + infrastructure adapters
                        |
                 nyanpasu-config
                 Profiles + executor + narrow ports

LuCI view → rpcd/session/ACL → ubusd → nyanpasu-openwrt adapter
                                            |
                                      router facade
                                            |
                          injected store/compiler/publisher/core ports
```

| Package                        | Responsibility                                                       |
| ------------------------------ | -------------------------------------------------------------------- |
| `backend/nyanpasu-config`      | Shared domain schema and pure pipeline execution                     |
| `backend/nyanpasu-application` | Shared runtime assembly, typed clients, workflows and consumed ports |
| `backend/nyanpasu-platform`    | Concrete script and infrastructure adapters                          |
| `backend/nyanpasu-openwrt`     | Daemon composition root, native ubus, filesystem and procd adapters  |
| `openwrt/nyanpasu`             | Daemon feed package and procd service definitions                    |
| `openwrt/luci-app-nyanpasu`    | Native LuCI view, menu and rpcd ACL                                  |

The OpenWrt binary must have no Tauri, egui/eframe, GTK, tray or widget dependency.
Its native dependencies are libubus, libubox and blobmsg JSON support. JS execution
retains Boa and Lua retains the vendored Lua engine; binary size and memory must
be measured, not estimated from the desktop application's size.

## Ownership and injected ports

The router `StateActor` owns the committed profile document and the last published
runtime candidate. Its mailbox serializes imports, selection, building and core
commands. Handlers await a whole operation before processing the next message;
there is no second scheduler or admission queue.

- `ConfigStore`: load/save one router document at a host-provided path.
- `RuntimeCompiler`: execute the shared builder against an immutable document.
- `RuntimePublisher`: validate a candidate with Mihomo and atomically publish it.
- `CoreControl`: inspect/start/stop the fixed procd service.

The facade exposes ordinary asynchronous methods. RPC adapters call the facade;
they do not manipulate files or processes directly. Root construction injects all
paths, concrete ports, script/cache directories and the root cancellation token.
The actor's state is not exposed through shared locks.

Blocking persistence and compilation run on `spawn_blocking`; panics propagate.
In-process actor calls have no timeout. A caller dropping its reply future does
not cancel work already owned by the actor. Shutdown cancels one root token,
refuses new work, and lets owner cleanup stop the core in `post_stop`.

procd owns Mihomo's process supervision and respawn. The Nyanpasu actor owns its
configuration and desired lifecycle commands. It observes procd's real state,
instead of setting an in-memory `running` flag after sending a start command.

## Persistence and failure semantics

Persist profile metadata, selected profile, script order and all source contents
in one versioned router document at `/etc/nyanpasu/state.json`. Use the existing
`Profiles` schema and durable revision. Profile IDs and managed content keys are
server-generated; a browser cannot choose a filesystem path.

An import parses a YAML mapping, enforces limits, assembles domain items and
validates references. A selection must name an existing config profile. Both
operations clone the committed document, increment its revision, atomically save
the complete candidate, and only then replace actor state. A failed save leaves
both the selected profile and in-memory contents unchanged.

The document's inline contents deliberately avoid multi-file transactions in
this MVP. They are not a compatibility format for desktop profiles. Future remote
sources and editable materialized files should reuse the desktop transaction
workflow after that workflow has been extracted, rather than adding a second
journal implementation here.

Build from a frozen committed snapshot, run actual JS/Lua transforms and reject
any transform failure. Do not publish a pipeline's partially successful fallback.
Serialize the resulting mapping, validate it with `mihomo -t`, then atomically
replace `/var/run/nyanpasu/runtime.yaml`. Failed compilation/validation/publication
preserves the previous runtime and returns a structured error.

`build_runtime` generates and publishes a candidate; it does not reload an
already-running core. `runtime_revision` denotes the published candidate, not
proof that a running process applied it. `start_core` refuses an already-running
core, rebuilds from the current committed revision, then starts and verifies it.
Changing profiles therefore requires an explicit stop/start to apply them.

After daemon restart, persisted profiles remain selected, while runtime inspection
starts empty and start rebuilds the candidate. Shutdown stops the core, so procd
does not run a stale configuration behind a fresh daemon. Core command errors are
reported after durable profile commits, without rolling back those commits.

## Native ubus protocol

Object: `nyanpasu`. Every operation is a named native ubus method. An optional
`request` blobmsg table contains the method DTO. Read methods also accept `{}`.
There is no `call(method,args)` operation that could expose arbitrary desktop
commands. Unknown methods and unexpected parameters are rejected.

| Method           | Kind     | Request                                      | Result                                        |
| ---------------- | -------- | -------------------------------------------- | --------------------------------------------- |
| `status`         | query    | `{}`                                         | `{running,current,revision,runtime_revision}` |
| `list_profiles`  | query    | `{}`                                         | `{current,items:[{id,name,script_count}]}`    |
| `import_profile` | mutation | `{name,content,scripts?:[{runtime,source}]}` | `{id}`                                        |
| `set_current`    | mutation | `{id}`                                       | `{current}`                                   |
| `get_runtime`    | query    | `{}`                                         | `{yaml,revision}` with nullable fields        |
| `build_runtime`  | mutation | `{}`                                         | `{yaml,revision}`                             |
| `start_core`     | mutation | `{}`                                         | `{running:true}`                              |
| `stop_core`      | mutation | `{}`                                         | `{running:false}`                             |

Example native request:

```sh
ubus call nyanpasu import_profile '{"request":{"name":"example","content":"proxies: []\nrules: [MATCH,DIRECT]\n","scripts":[]}}'
ubus call nyanpasu list_profiles '{}'
```

Script runtime names are `javascript` and `lua`. A JS transform defines
`function main(config) { ...; return config }`. A Lua transform reads `config`
and returns a mapping, for example `config.mode = 'direct'; return config`.

Replies use `{result: <typed value>}` or `{error: {kind,message,...}}`. A domain
failure is never placed inside a successful result. LuCI must unwrap the envelope
and render errors. This envelope is application error data carried by ubus;
it is not JSON-RPC. Native ubus failures remain ubus status errors.

New router handlers use an explicit `#[nyanpasu_macro::rpc(ubus)]` expansion that
generates parsing/registration without Tauri types. Existing desktop/HTTP macro
expansion and metadata stay unchanged. The router inventory, C method table,
query/mutation classification and ACL must agree and be checked together.

The C shim does only libubus registration, uloop lifecycle and blobmsg-to-JSON
conversion. Each server context owns its callback and object. Rust owns the
dispatcher and facade. No mutable singleton stores the application or callback.
SIGINT/SIGTERM end uloop; Rust then cancels the root token and awaits actor cleanup.
ubus disconnection is an exit/failure for procd to restart, not false readiness.

MVP limits are 32 KiB of raw content per imported profile, eight scripts per
profile and 64 domain items including scripts. JSON escaping and final config
size also count toward native ubus frame capacity. The adapter must reject an
oversized request/reply explicitly; this first version is for small profiles,
not large subscription sets or streaming. A future upload/resource protocol can
remove this limit without assuming ubus supports arbitrary large messages.

## LuCI, authorization and host adapters

Use a native LuCI view for the first version. The current React distribution is
about 27 MiB and its startup/pages include native Tauri APIs. A new transport alone
does not make those pages platform-independent. The LuCI view is a host adapter,
using LuCI controls, translation, sessions, notifications and polling; it contains
no config-generation or process-control business logic.

Mount the view at `admin/services/nyanpasu`. It has a profile selector, YAML
input, ordered optional script inputs, runtime preview, explicit Build/Start/Stop
actions, actual status and visible errors. Disable actions while they are pending;
read-only LuCI sessions must not display writable controls as usable.

The ACL grants queries (`status`, `list_profiles`, `get_runtime`) as read and
mutations individually as write. No wildcard permission, unauthenticated new HTTP
listener, user-supplied session identity, shell command, executable path or
filesystem path is accepted from RPC. LuCI/rpcd enforce session authentication and
capabilities; the daemon trusts local ubus access like other privileged services.

Two fixed procd services are installed:

- `nyanpasu`: runs the foreground daemon and sends stdout/stderr to logd.
- `nyanpasu-core`: runs `/usr/bin/mihomo -d /var/lib/nyanpasu/core -f
/var/run/nyanpasu/runtime.yaml`, with procd supervision.

Only the daemon is boot-enabled. The core starts through the application command
after a successful build, so independent boot scripts cannot bypass validation.
Core control invokes fixed argv, never a shell constructed from RPC content.
Root CLI may supply alternate paths for tests; browser DTOs cannot.

Router policy finalization disables TUN, DNS listening, redir/tproxy listeners and
non-loopback controller endpoints regardless of imported source or script output.
Do not let a desktop-shaped profile accidentally take over router networking.
Manual routing/firewall and LAN-listener behavior need an explicit next design.

## Build, package and local verification

The native-ubus feature links libubus/libubox on Linux. Default library builds on
macOS support dispatcher and injected actor tests without pretending to have a
native ubus service. A portable build of the daemon must fail clearly if native
ubus is unavailable.

Build release binaries separately for `aarch64-unknown-linux-musl` and
`x86_64-unknown-linux-musl`; no desktop-wide hardware AES baseline should be added
to these router targets. Prefer a matching ImmortalWrt/OpenWrt SDK for final feed
packages. A local Docker musl builder plus matching native libraries can exercise
the MVP, but does not establish SDK/package ABI compatibility by itself.

Feed Makefiles package a supplied target binary and install the init scripts,
LuCI view/menu and ACL. Reject a missing binary rather than silently building an
empty package. Record how the binary was produced and verify its architecture,
interpreter and shared-library dependencies. An SDK package build and `opkg`
installation are separate evidence from copying files into a test container.

Docker verification must use an isolated container with no host networking,
router LAN bridges or host filesystem writes except an explicit build/artifact
directory. Bind any test LuCI port only to host loopback. Do not enable container
firewall/DNS/routing changes on the user's actual router.

Acceptance evidence:

1. Shared runtime/script tests pass, and the daemon dependency tree excludes GUI.
2. Injected actor tests prove persistence failure leaves state unchanged, bad
   selection is rejected, failed transforms/publication preserve runtime, profile
   restart works, and lifecycle operations observe the injected host.
3. Native feature compiles and links in a Linux musl environment.
4. In ImmortalWrt/OpenWrt, `ubus list nyanpasu -v` enumerates all eight methods;
   native calls import/select/build scripts and return errors as specified.
5. A real Mihomo binary validates and starts with the generated config, procd
   reports running, then stop and daemon shutdown leave no core process.
6. Authenticated LuCI shows the view and executes the workflow. Unauthenticated
   and read-only sessions cannot perform mutations; displayed controls agree.
7. Both target builds are attempted; report each target's actual result. Docker
   process evidence is not physical-router, firewall, SDK or production evidence.
8. Desktop caller checks and architecture gates identify extraction regressions.

Repository test/build orchestration belongs under `scripts/src/` and is exposed
through named `deno task` entries. Do not create a separate Node/shell test framework.

## Implementation sequence and next milestones

1. Land this design and contracts before further implementation.
2. Extract shared runtime/scripts and remove the model-to-egui dependency;
   verify desktop imports and shared pipeline behavior.
3. Add the injected router state owner and storage/runtime/core adapters;
   verify durable state and terminal operation behavior with fakes.
4. Add native ubus transport, typed handlers, LuCI and procd packaging;
   verify API/ACL agreement and structured failures.
5. Build and exercise the real daemon/core in Docker, then record limitations and
   exact reproduction commands in the package README.

After this MVP, extract desktop transactional profile/state workflows rather
than growing a parallel router implementation. Then move shared facade/RPC
metadata into neutral packages, add subscription/resource transfers and selected
frontend features, and only then design firewall/DNS integration with rollback,
reboot recovery and coexistence policies. Full React embedding is evaluated after
native capability guards and router API coverage have matured.

## Primary references

- [ImmortalWrt official Docker tooling](https://github.com/immortalwrt/docker)
- [OpenWrt official Docker tooling](https://github.com/openwrt/docker)
- [libubus server API](https://github.com/openwrt/ubus/blob/master/libubus.h)
- [LuCI RPC/view example](https://github.com/openwrt/luci/blob/master/applications/luci-app-example/htdocs/luci-static/resources/view/example/rpc.js)
- [LuCI ACL example](https://github.com/openwrt/luci/blob/master/applications/luci-app-example/root/usr/share/rpcd/acl.d/luci-app-example.json)
- [procd init scripts](https://openwrt.org/docs/guide-developer/procd-init-script-example)
- [OpenWrt SDK](https://openwrt.org/docs/guide-developer/toolchain/using_the_sdk)
