# Shared core and platform extraction

Status: target revised by user, 2026-10-08. NyanpasuClient and the shared
application layer move into `nyanpasu-core`, matching the upstream core/shell
extraction PRs. A separate `nyanpasu-application` crate is not part of the target.
The filename is retained so existing plan links remain valid.
[Backend package standards](../development/backend-packages.md) define ownership.

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

[#5652](https://github.com/libnyanpasu/clash-nyanpasu/pull/5652) delivered runtime
builder/builtins in `nyanpasu-application`, filesystem/Boa/Lua in platform and the
widget wire enum in helper. This is existing source behavior to reuse, not work
to repeat. The target change does not mean that consolidation has already landed.

Upstream extraction migrates application-owned behavior into core, updates
platform and desktop consumers, then removes the obsolete application crate,
Cargo dependencies and lockfile entries in complete buildable units. Do not keep
old-path re-exports or a second facade solely to preserve imports. Keep existing
transitional dependencies GUI-free and avoid dependency cycles until migration.

[#5621](https://github.com/libnyanpasu/clash-nyanpasu/pull/5621) and
[#5629](https://github.com/libnyanpasu/clash-nyanpasu/pull/5629) establish the
core extraction/gate route. Reconcile the runtime error/preparation contract and
reuse accepted rates/compat implementations. Any session-state follow-up such as
[#5666](https://github.com/libnyanpasu/clash-nyanpasu/pull/5666) must align its
ultimate owner with core; an application-targeted historical patch is not the
final package direction. These links identify upstream work, not merge commands.

Local facade WIP is source material only. Recover touched behavior against the
accepted main baseline, preserving newer MeowAlpha/version APIs, tests and wire
contracts. Do not replay historical commit ranges or test counts as current proof.

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

OpenWrt remains paused pending upstream core and shell/Tauri separation. Its
[roadmap](openwrt-roadmap.md) does not reorder or implement these upstream PRs.
After upstream completion and explicit user resume, reassess main and compose
NyanpasuClient from core with router adapters. Do not add a router profile store,
mutation journal or parallel application facade to bypass missing core behavior.

Shared musl tests are not daemon, native ubus, procd, SDK package or hardware
acceptance. Validate these separately after resume, including native dependencies,
CPU requirements and JS/Lua resource costs. The
[MVP definition](openwrt-mvp.md) retains the product scope and deferred policies.
