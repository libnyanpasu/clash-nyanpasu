# Shared core and platform extraction

Status: core/GUI split delivered through Part 23 (M9, `beebb7802`).
`NyanpasuClient` and the shared application layer live in `nyanpasu-core`;
concrete infrastructure lives in `nyanpasu-platform`. [Backend package
standards](../development/backend-packages.md) define ownership.

## Target ownership

```text
Tauri / desktop shell             OpenWrt host (paused)
  presentation + transport         ubus + router composition
                  \               /
                   nyanpasu-core
                   NyanpasuClient, typed clients, actors,
                   use cases, workflows, consumed ports,
                   state transaction machinery
                             |
                   nyanpasu-config / independent domain crates

Hosts inject concrete nyanpasu-platform / infrastructure implementations.
Core does not import platform, Tauri or desktop presentation libraries.
```

Shared application decisions, profile/configuration transactions, runtime build,
core control, receipts, recovery and observations belong in core. Config retains
schemas, pure execution and its consumer-owned ports. Platform contains concrete
filesystem/script/process/network/OS implementations. Tauri-specific behavior
stays with shell/host adapters, including windows/tray/widgets, main-thread
presentation, desktop plugins, IPC and HTTP delivery.

Classify each component by responsibility, not merely by absence of Tauri types.
Headless startup must not require fake GUI dependencies or successful no-op
business effects. Keep explicit availability/outcomes for optional capabilities.

## Delivered baseline versus target

The split is delivered in the current tree: `backend/nyanpasu-core/src/client/`
owns the facade, typed clients and workflows; the sibling domain modules own
their actors, services and transactions.
`backend/nyanpasu-platform/src/profiles/`, `enhance/runtime_build.rs` and
`changelog_http.rs` own the concrete profile file, runtime-build and changelog
HTTP implementations. `backend/tauri/src/setup.rs` constructs and injects
platform implementations and GUI adapters. Tauri retains presentation, desktop
plugins, frontend events, IPC and HTTP transport.

The runtime builder and builtins previously extracted into
`nyanpasu-application` are consolidated in `nyanpasu-core::enhance`. Desktop and
platform callers use core directly; there is no separate application crate.

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

Part 24 (M10) documents this layout. The next goal is `nyanpasu-cli`/headless
assembly.

## Migration requirements

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
