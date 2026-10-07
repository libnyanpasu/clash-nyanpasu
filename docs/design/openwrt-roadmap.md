# OpenWrt downstream roadmap

Status: paused by user, 2026-10-08. No OpenWrt implementation is scheduled.
Resume only when the user requests it after the upstream shared-backend and
shell/Tauri separation work is complete. Upstream completion alone does not
implicitly resume this project. No monitoring automation is requested.

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

The current upstream plan moves NyanpasuClient, shared use cases, typed clients,
actors, workflows and consumed ports into `nyanpasu-core`. No separate
`nyanpasu-application` layer is required in the target. Concrete reusable adapters
remain in platform or existing infrastructure crates, injected by the host;
core must not depend on platform or a frontend. Config retains its domain models
and pure executor. Tauri/shell remains the desktop host and presentation boundary.

The runtime behavior delivered by #5652 now lives in `nyanpasu-core::enhance`,
with platform/desktop consumers migrated and the former application crate removed.
Follow-up actor/facade extraction targets core directly. This is shared-backend
consolidation, not resumed OpenWrt work. OpenWrt still waits for upstream core/shell
completion and introduces no second application facade.

On resume, map the router to the actual accepted core APIs on main. Headless
construction and operation are readiness evidence; a renamed crate alone is not.

## Upstream dependency snapshot

Snapshot verified on 2026-10-08; refresh on resume.

| Work                                                                                                                               | State                                                                                                                                                 | Treatment while paused                                                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| [#5652](https://github.com/libnyanpasu/clash-nyanpasu/pull/5652)                                                                   | Already merged on 2026-10-06; runtime builder/scripts and neutral config baseline; runtime consolidated into core; original application crate removed | Delivered foundation. Waiting means its related follow-up work, not waiting for this PR to merge again.                    |
| [#5621](https://github.com/libnyanpasu/clash-nyanpasu/pull/5621), [#5629](https://github.com/libnyanpasu/clash-nyanpasu/pull/5629) | Open architecture/pure-service work                                                                                                                   | Upstream-owned review, architecture reconciliation and integration. No OpenWrt-mandated merge order.                       |
| [#5666](https://github.com/libnyanpasu/clash-nyanpasu/pull/5666)                                                                   | Open session-state work                                                                                                                               | Upstream-owned. Desktop session-state extraction alone does not establish router readiness.                                |
| Shell/Tauri separation                                                                                                             | Discussion stage: no dedicated implementation/PR identified in project docs and PR search                                                             | Required external dependency. Wait for an implemented, reviewed result on main; no invented PR number or completion claim. |
| [#5645](https://github.com/libnyanpasu/clash-nyanpasu/pull/5645)                                                                   | Merged frontend-independent path resolution                                                                                                           | Reuse explicit path inputs; this does not prove complete shell separation.                                                 |
| Local application-facade WIP                                                                                                       | Historical unfinished extraction                                                                                                                      | Preserve as reference. Do not replay its old commits into a moving main or implement its contracts in advance.             |

Paused work includes daemon composition, native ubus, procd endpoints, LuCI/ACL,
feed packaging, and OpenWrt-specific musl/SDK acceptance. Existing generic
backend/shell work remains separate and may proceed under its own authorization.
There is no date estimate or active OpenWrt PR queue.

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
| Native management     | Add explicit ubus methods with a narrow native bridge, then LuCI and per-method ACL                          | Native calls plus authenticated LuCI loop; unauthorized mutations denied; errors, pending actions and status observable.                                                 |
| Package acceptance    | Target-specific SDK/feed builds and installation in the chosen test environment                              | Both aarch64/x86_64 results recorded; native library ABI, package install, procd and LuCI verified. Physical-router support remains unclaimed without hardware evidence. |

These are future milestones, not fixed PR counts or frozen interfaces. Split
reviewable functional units after reassessment; keep each independently buildable.
Do not assume the previous eight-method DTO table, rpc(ubus) macro proposal,
filesystem layout, service names or stop/start convention are accepted contracts.
Select the thinnest adapter consistent with the completed shared application.

## Decisions deliberately deferred

Before lifecycle/authorization implementation, resolve daemon-crash/core retention
and read-only access to runtime YAML containing credentials. These questions are
not blockers for pausing and must not be answered by silence. Also finalize
script capability/resource limits, payload limits, SDK/release targets and
profile-to-runtime apply semantics using the upstream APIs and target evidence.

The first product remains a native ubus daemon with native LuCI for small local
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
- [rpcd ucode](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/luci-app-nikki/root/usr/share/rpcd/ucode/luci.nikki) and [ACL](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/luci-app-nikki/root/usr/share/rpcd/acl.d/luci-app-nikki.json): native LuCI integration reference. Its ucode RPC and wildcard/file permissions are not our fixed Rust-daemon ubus contract; retain explicit method permissions and shared facade calls.
- [SDK workflow](https://github.com/nikkinikki-org/OpenWrt-nikki/blob/7b203f6c4c5e94c6c0026acb301090aa1d310e7f/.github/workflows/build-packages.yml): release/architecture matrix and separate package artifacts. Start with the agreed two targets, and select opkg/apk packaging by the chosen release rather than assuming one package manager universally.

Nikki's networking/transparent-proxy integration is a later reference track,
not an MVP dependency. Do not adopt its firewall/kernel-module dependencies,
UCI profile processing, routing takeover or generic core API forwarding merely
because they are present. Preserve shared profile/script/transaction ownership.
Before any future code reuse, check its GPL license and source attribution;
this reassessment only records design references, with no copied source code.
