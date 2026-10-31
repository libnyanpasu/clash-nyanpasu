# Shared core and platform extraction

Status: the Tauri extraction baseline is delivered through Part 24
([#6108](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108)).
`NyanpasuClient` and the shared application layer live in `nyanpasu-core`, and
core no longer depends on Tauri. Package ownership is not final: the target
below replaces the earlier definition of `nyanpasu-platform` as the home for
every concrete filesystem, script, process, network and OS implementation.
[Backend package standards](../development/backend-packages.md) define the
rules that apply today.

## Target ownership

```text
Hosts (composition roots)
  Tauri desktop shell   nyanpasu-cli (planned)   OpenWrt host (paused)   mobile (later)
  each host owns its presentation capability implementations
           \                  |                      |                   /
            +--- inject capability, OS and infrastructure implementations ---+
                                    |
                              nyanpasu-core
                              NyanpasuClient, typed clients, actors, use cases,
                              workflows, consumed ports, optional capabilities
                                    |
                              nyanpasu-profile-transform (planned)
                              runtime builder, builtin transforms, artifacts
                                    |
                              nyanpasu-config
                              schemas, pure runtime executor and its ports

Implementation crates, injected by hosts:
  nyanpasu-scripts (planned)   script runners and extension definitions
  nyanpasu-os (planned)        OS integration capabilities
  nyanpasu-platform            portable infrastructure; to be renamed

Core does not import implementation crates, Tauri or presentation libraries.
```

Crate names marked planned are working names; the PR that creates each crate
settles its name.

Shared application decisions, profile/configuration transactions, core control,
receipts, recovery and observations belong in core. Config retains schemas, pure
execution and its consumer-owned ports. Classify each component by
responsibility, not merely by absence of Tauri types. Headless startup must not
require fake GUI dependencies or successful no-op business effects. Keep
explicit availability/outcomes for optional capabilities.

### Script runtimes and profile transform

Script runtimes are part of profile enhancement, not platform infrastructure.

- The script crate provides the JavaScript and Lua runners and their extension
  definitions. It depends only on the `nyanpasu-config` ports `ScriptRunner` and
  `ProfileContentSource` (`runtime/executor/ports.rs`), not on core.
- The runtime builder, builtin transforms and artifact snapshots move from
  `nyanpasu-core::enhance` to `nyanpasu-profile-transform`. Core depends on the
  transform crate.
- The transform crate does not depend on the script crate. The host injects the
  runner, so the script engines compile in parallel with transform and core, and
  hosts can leave out an engine through features.
- What a script may reach is part of its extension definition and is injected:
  module loading (including the JavaScript HTTP module loader), network,
  filesystem and console. Runners keep the `ScriptRunner` determinism contract.

### System capabilities

System capabilities vary along two independent axes and are owned accordingly.

| Kind                       | Varies with                             | Capabilities                                                                                                          | Owner                                     |
| -------------------------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| Presentation / interaction | Host form: desktop, mobile, CLI, server | Global hotkeys, windows, tray, widgets, dialogs, notifications, clipboard, opener, deep links, single instance        | Each host                                 |
| OS integration             | Operating system and privileges         | System proxy, PAC, autostart, service install and control, DNS flush, device identity, core binary install, elevation | OS crate, used by GUI and CLI hosts alike |

- Presentation capabilities are optional in core. A host that lacks one passes
  none, and the dependent operation reports it as unsupported.
- A CLI on a desktop operating system needs the OS integration capabilities as
  much as the GUI does, so they do not live in the GUI crate.
- Dialogs are host behavior and have no core port. Elevation is the exception
  that crosses both axes: the OS crate owns the mechanism, and the host injects
  how consent is obtained (an authorization dialog on a GUI host, a terminal
  prompt on a CLI, refusal on a server). Core never shows an OS dialog itself.

### Portable infrastructure

Profile files, subscription and changelog HTTP, egress probes, updater
downloads, file watchers and storage backends behave the same on every host.
They are injected implementations, but not platform capabilities. The remaining
`nyanpasu-platform` crate holds this group and is renamed when the script and
transform code has moved out (working name `nyanpasu-io`).

## Delivered baseline versus target

The Tauri extraction is delivered in the current tree:
`backend/nyanpasu-core/src/client/` owns the facade, typed clients and
workflows; the sibling domain modules own their actors, services and
transactions. `backend/nyanpasu-platform/src/profiles/`,
`enhance/runtime_build.rs` and `changelog_http.rs` own the concrete profile
file, runtime-build and changelog HTTP implementations.
`backend/tauri/src/setup.rs` constructs and injects these implementations and
the GUI adapters. Tauri retains presentation, desktop plugins, frontend events,
IPC and HTTP transport.

The runtime builder and builtins previously extracted into
`nyanpasu-application` are consolidated in `nyanpasu-core::enhance`. Desktop and
platform callers use core directly; there is no separate application crate.

Known gaps against the target ownership:

- Script runners live in `nyanpasu-platform/src/enhance/script/`. Their only
  core imports are `ScriptType` and `ScriptWrapper` from
  `nyanpasu-core::enhance`. `FsRuntimeBuildAdapter` constructs the runner itself
  instead of receiving it.
- OS integration implementations are split between core and the Tauri crate.
  In core: service install and control with elevation (`service/control.rs`,
  `utils/sudo.rs`, `actor_v2/service_host_adapter.rs`), DNS flush
  (`client/system_dns.rs`), core binary install
  (`client/core_lifecycle/adapters.rs`), device identity (`utils/hwid.rs`) and
  system proxy reads (`utils/config.rs`). In Tauri: system proxy, PAC and
  autostart (`adapters/system_proxy.rs`) and the sidecar core version reader
  (`utils/core_version.rs`). Core's service install and DNS flush show OS
  authorization dialogs directly.
- `ClientSetupArgs` requires `WindowControl`, `AcceleratorValidator` and a
  `DebugHttpFactory`, so a headless host must pass stand-ins.
- The updater backend is constructed inside the facade, runtime candidate file
  IO is implemented by `RuntimePaths`, and the traffic store is typed to the
  redb backend in `ClientSetupArgs`.
- Effect execution for hotkeys, tray, widgets and locale, and the main-thread
  handoff, still live in core.

### Core split stack

The following parts are present through Part 23.

| Part    | PR / commit                                                      | Change                                                 |
| ------- | ---------------------------------------------------------------- | ------------------------------------------------------ |
| 01      | [#6065](https://github.com/libnyanpasu/clash-nyanpasu/pull/6065) | Keep bundle host logic out of client code              |
| 02      | [#6066](https://github.com/libnyanpasu/clash-nyanpasu/pull/6066) | Extract owner shutdown helpers                         |
| 03      | [#6068](https://github.com/libnyanpasu/clash-nyanpasu/pull/6068) | Separate YAML and key-value parsing from GUI helpers   |
| 04      | [#6071](https://github.com/libnyanpasu/clash-nyanpasu/pull/6071) | Inject host build metadata                             |
| 05      | [#6072](https://github.com/libnyanpasu/clash-nyanpasu/pull/6072) | Move presentation ports to their effects consumer      |
| 06      | [#6073](https://github.com/libnyanpasu/clash-nyanpasu/pull/6073) | Separate Tauri delivery adapters from shared ports     |
| 07      | [#6074](https://github.com/libnyanpasu/clash-nyanpasu/pull/6074) | Gather GUI adapters under `adapters/`                  |
| 08      | [#6075](https://github.com/libnyanpasu/clash-nyanpasu/pull/6075) | Own core status events at the GUI boundary             |
| 09      | [#6076](https://github.com/libnyanpasu/clash-nyanpasu/pull/6076) | Own stream and storage events at the GUI boundary      |
| 10      | [#6077](https://github.com/libnyanpasu/clash-nyanpasu/pull/6077) | Own client notification events at the GUI boundary     |
| 11      | [#6078](https://github.com/libnyanpasu/clash-nyanpasu/pull/6078) | Split profile file ports, helpers and concrete service |
| 12a     | [#6079](https://github.com/libnyanpasu/clash-nyanpasu/pull/6079) | Inject concrete adapters from the composition root     |
| 12b     | [#6080](https://github.com/libnyanpasu/clash-nyanpasu/pull/6080) | Inject the host runtime-build port                     |
| 13      | [#6081](https://github.com/libnyanpasu/clash-nyanpasu/pull/6081) | Run application tests with Tokio                       |
| 14a     | [#6082](https://github.com/libnyanpasu/clash-nyanpasu/pull/6082) | Give shared fixtures independently movable owners      |
| 14b     | [#6085](https://github.com/libnyanpasu/clash-nyanpasu/pull/6085) | Keep platform test subjects at host boundaries         |
| 15 (M1) | [#6086](https://github.com/libnyanpasu/clash-nyanpasu/pull/6086) | Move leaf utilities into core                          |
| 16 (M2) | [#6087](https://github.com/libnyanpasu/clash-nyanpasu/pull/6087) | Move service control and downloads into core           |
| 17 (M3) | [#6088](https://github.com/libnyanpasu/clash-nyanpasu/pull/6088) | Move storage, backups and migrations into core         |
| 18 (M4) | [#6090](https://github.com/libnyanpasu/clash-nyanpasu/pull/6090) | Move the core control plane into core                  |
| 18b     | [#6092](https://github.com/libnyanpasu/clash-nyanpasu/pull/6092) | Prepare log and geodata fixtures for extraction        |
| 19 (M5) | [#6093](https://github.com/libnyanpasu/clash-nyanpasu/pull/6093) | Move core logs and geodata into core                   |
| 19b     | [#6094](https://github.com/libnyanpasu/clash-nyanpasu/pull/6094) | Keep connection-detail subscription state in the GUI   |
| 20 (M6) | [#6102](https://github.com/libnyanpasu/clash-nyanpasu/pull/6102) | Move the Clash API client and streams into core        |
| 21 (M7) | [#6103](https://github.com/libnyanpasu/clash-nyanpasu/pull/6103) | Move traffic accounting into core                      |
| 21b     | [#6105](https://github.com/libnyanpasu/clash-nyanpasu/pull/6105) | Isolate profile file integration test ownership        |
| 22 (M8) | [#6106](https://github.com/libnyanpasu/clash-nyanpasu/pull/6106) | Move the facade and transaction graph into core        |
| 23 (M9) | [#6107](https://github.com/libnyanpasu/clash-nyanpasu/pull/6107) | Move concrete infrastructure adapters into platform    |
| 24      | [#6108](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108) | Document the core and GUI crate split                  |

Part 24 (M10) documents the delivered layout. Review of that PR recorded the ownership
correction above
([comment](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108#issuecomment-6545500519)).

### Next stack

The next goal is `nyanpasu-cli`/headless assembly. The ownership correction comes
first, in this order:

1. Extract the script crate: move `ScriptType` and `ScriptWrapper` down and
   inject the runner into the runtime-build adapter.
2. Extract `nyanpasu-profile-transform` from `nyanpasu-core::enhance`.
3. Make presentation capabilities and `DebugHttpFactory` optional in
   `ClientSetupArgs`, with explicit unsupported outcomes. Decide whether the
   capability contracts stay in core or move to a small contract crate that lets
   implementation crates compile in parallel with core.
4. Create the OS crate with an elevation consent port, and move the OS
   integration implementations out of core and the Tauri crate.
5. Rename the remaining platform crate to reflect portable infrastructure, and
   inject the updater backend, runtime candidate IO and storage backends.
6. Split shared convergence from presentation effects, then assemble the
   headless CLI.

Extend `deno task lint:backend-boundaries` as each crate appears, so the
direction rules in [Target ownership](#target-ownership) stay enforced.

## Migration requirements

- Do not add script runtime or OS integration code to `nyanpasu-platform`, and do
  not add mandatory presentation capabilities to `ClientSetupArgs`. New code
  follows the target ownership; if its crate does not exist yet, keep the code
  behind a consumer-owned port next to its current consumer.
- Move complete bottom-up call paths and update desktop consumers in each PR.
- Preserve actor ownership, generation fences, instance API leases, caller-drop
  behavior, real in-process replies, panic semantics and orderly shutdown.
- Move profile/configuration owners and their runtime mutation participant as a
  coherent transaction boundary. Preserve voting, commit, materialization,
  promotion/compensation, degradation, retry and restart reconciliation.
- Split shared convergence from presentation effects before headless assembly.
  Frontend invalidation hints are not commit/effect receipts; preserve actual
  owner observation intent and lifecycle refresh behavior at transport bridges.
- Inject explicit paths, script/content sources, runtime candidate IO, validation
  and process control. Shared core construction uses ordinary async/Tokio;
  no Tauri runtime, frontend server/assets or GUI capability is mandatory.
- Run relevant existing behavior/golden tests, desktop checks, formatting/Clippy,
  architecture and dependency gates. Extend source scanning to migrated owners.
  Preserve structured errors and serialization; regenerate changed bindings.

## OpenWrt integration boundary

OpenWrt remains paused. Its
[roadmap](openwrt-roadmap.md) does not reorder or implement these upstream PRs.
After explicit user resume, reassess main and compose
NyanpasuClient from core with router adapters. Do not add a router profile store,
mutation journal or parallel application facade to bypass missing core behavior.

Shared musl tests are not daemon, native ubus, procd, SDK package or hardware
acceptance. Validate these separately after resume, including native dependencies,
CPU requirements and JS/Lua resource costs. The
[MVP definition](openwrt-mvp.md) retains the product scope and deferred policies.
