# Mobile shells and privileged service modes: tasks

Milestone cards for the [design](design.md). Each card is refined to
task-level detail and reviewed before work on it starts, as in the
[core package split tasks](../2026-11-03-core-package-split/tasks.md).
Cards marked _runtime_ are PRs in
[libnyanpasu/nyanpasu-runtime](https://github.com/libnyanpasu/nyanpasu-runtime),
landed here as a gitlink update on a released tag.

## Global constraints

- No new Tauri dependency in core, the target crates or any neutral crate; new
  crates are registered in `lint:backend-boundaries`, the static gate prefixes
  and, where they build on Linux, the musl test runner.
- Desktop behavior does not change. Each card that touches shared code runs the
  desktop checks from [Testing and review](../../development/testing.md) and
  keeps generated bindings without diff.
- Mobile ports reuse `ServiceProcessControl`, `ElevationConsent` and
  `ControlEndpoint`; a card that needs a new port records why the existing one
  does not fit.

## Cards

| Card | Content                                                                                                                                                                 | Depends on                      | Done when                                                                                                                              |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| M0   | Build probe: `cargo check` of the core family, implementation crates and `nyanpasu-ipc` for `aarch64-linux-android` and `aarch64-apple-ios`                             | —                               | A recorded list of failing crates and causes; no code change                                                                           |
| M1   | Spike on a rooted device: module with `service.sh` and `sepolicy.rule` starts a test daemon; the app connects over an abstract socket under KernelSU, Magisk and APatch | —                               | Working SELinux rules per manager recorded in the design; O-3 decided                                                                  |
| M2   | _runtime_: per-platform IPC endpoint policy (name, creation, peer authorization); Android abstract socket with `SO_PEERCRED`                                            | M1                              | Desktop endpoints unchanged (existing tests); Android peer authorization tested with an allowed and a rejected UID                     |
| M3   | _runtime_: `nyanpasu-service` builds for Android and runs in the foreground without `service-manager`                                                                   | M2                              | Daemon starts under `service.sh` on a device and serves the v2 API; desktop install paths unchanged                                    |
| M4   | Module packaging: `module.prop`, `service.sh`, `sepolicy.rule` and core binaries assembled into a zip by a repository script, built in CI                               | M3                              | `pnpm run` entrypoint produces an installable zip; CI artifact; follows [Repository scripts](../../development/scripts.md)             |
| M5   | Mobile shell skeleton: Tauri mobile crate, composition root with neutral sinks and no presentation capabilities, Android project                                        | D1–D4, A2b, E1, E2; O-1 decided | App starts on an emulator in unprivileged mode with no core running; shared queries answer; presentation commands return `unsupported` |
| M6   | `nyanpasu-android`: root detection, `su`-based `ServiceProcessControl`, consent policy, module install through the manager CLI                                          | M4, M5                          | Port tests with fake command runners; refusing consent on an unrooted device verified before any `su` call                             |
| M7   | Android root mode end to end: service mode on, daemon capture via TUN with package filters                                                                              | M6                              | On a rooted device: install, reboot, start, switch profiles, stop, uninstall; the two modes never capture at once                      |
| M8   | iOS spike on a jailbroken device and under TrollStore: daemon or root helper, `utun` capture, socket reachability from the app                                          | M2                              | O-4 decided; the design's iOS section updated with the verified mechanism                                                              |
| M9   | Android `VpnService` mode: refine design §7 into its own spec, then implement                                                                                           | M5; O-5 decided                 | Separate spec reviewed                                                                                                                 |
| M10  | iOS shell and NetworkExtension mode: separate spec                                                                                                                      | M5, M8                          | Separate spec reviewed                                                                                                                 |

M0–M4 and M8 can proceed while the core split stack is in review. M5 waits for
that stack. Mobile UI work waits for #6257 and has its own spec.
