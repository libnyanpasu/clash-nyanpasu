# Mobile shells and privileged service modes

**Date:** 2026-11-10

**Status:** Draft proposal. Not yet reviewed; no implementation has started.

**Baseline:** main `9307694a0`, `backend/nyanpasu-runtime` at `a27c3fc`.

**Authority order:** current `AGENTS.md` and development guides >
[shared core, shells and package split](../../design/application-platform-extraction.md) >
[core package split spec](../2026-11-03-core-package-split/design.md) > this spec >
[tasks](tasks.md).

## 1. Decisions

1. Android and iOS are served by a **mobile shell**, the "mobile shell (later)"
   of the [target ownership](../../design/application-platform-extraction.md#target-ownership).
   It is a composition root like `backend/tauri`, runs core in process and
   serves the existing frontend over Tauri IPC.
2. Each mobile platform has two operating modes, matching the desktop split
   between the local host and the service host
   (`ExecutionHost::{Local, Service}`, `nyanpasu-core/src/actor_v2/endpoint.rs:34`):

   | Platform | Unprivileged mode (`Local`)           | Privileged mode (`Service`)                                     |
   | -------- | ------------------------------------- | --------------------------------------------------------------- |
   | Android  | `VpnService`, core started by the app | Root daemon installed as a KernelSU, Magisk or APatch module    |
   | iOS      | NetworkExtension packet tunnel        | Root daemon on a jailbroken device, or a TrollStore root helper |

3. The privileged modes reuse the existing service path: `ServiceEndpoint`
   (`endpoint.rs:415`), the `nyanpasu-ipc` v2 API, `nyanpasu-service` and the
   `ServiceProcessControl` and `ElevationConsent` ports
   (`nyanpasu-contracts/src/service.rs:113`, `nyanpasu-contracts/src/elevation.rs:38`).
   Core gains no mobile-specific branch; the mobile target crates implement
   these ports and the shell injects them.
4. **Android root mode is the first mobile milestone.** It needs no new core
   control implementation, only a new daemon host, IPC endpoint policy and
   packaging. `VpnService` mode follows; the iOS modes come last.
5. iOS on a stock device, including the Simulator and development builds, has
   no privileged mode. The sandbox allows neither daemons nor root helpers, so
   NetworkExtension is the only route there. The iOS privileged mode is for
   jailbroken devices and testing, outside the App Store.
6. The mobile shell starts only after D1–D4, A2b, E1 and E2 of the core package
   split have landed (§8). Runtime and packaging work in `nyanpasu-runtime`
   (§5.1–§5.3) may start earlier because it does not touch that stack.

## 2. Current facts

| Area                 | Fact                                                                                                                                                                                                                                       | Source                                                                                                         |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| Tauri crate          | Builds as `staticlib`, `cdylib` and `rlib`; `run()` carries `#[cfg_attr(mobile, tauri::mobile_entry_point)]`. Only `tauri-plugin-global-shortcut` is excluded on mobile; Slint widget, crash reporter and `nyanpasu-os` are always linked. | `backend/tauri/Cargo.toml:12-14,231-232`; `backend/tauri/src/lib.rs:146`                                       |
| Mobile project       | No `gen/android` or `gen/apple` project and no mobile section in `tauri.conf.json`.                                                                                                                                                        | `backend/tauri/`                                                                                               |
| Toolchain            | Not verified: no Android or iOS Rust target has been built for any backend crate.                                                                                                                                                          | —                                                                                                              |
| Core control         | `ControlEndpoint` has two implementations, `LocalEndpoint` and `ServiceEndpoint`; the router consumes a trait object plus an `ExecutionHost` tag.                                                                                          | `nyanpasu-core/src/actor_v2/endpoint.rs:112,166,415`                                                           |
| Local core process   | The core manager starts the core as a child process from `spec.core.binary_path`.                                                                                                                                                          | `nyanpasu-core-manager/src/instance.rs:638`                                                                    |
| Core dependencies    | `nyanpasu-core` depends on `nyanpasu-core-manager` and `nyanpasu-ipc` unconditionally.                                                                                                                                                     | `backend/nyanpasu-core/Cargo.toml:56,65`                                                                       |
| Service ports        | `ServiceProcessControl` covers probe, install, uninstall, start, stop and update. `ElevationConsent` decides synchronously whether an operation may request OS authorization.                                                              | `nyanpasu-contracts/src/service.rs:113-121`; `nyanpasu-contracts/src/elevation.rs`                             |
| IPC endpoint         | Unix endpoints are fixed at `/var/run/{name}.sock`. After bind, the server runs `chown root:nyanpasu` and `chmod 664` as external commands. Listener mode cfgs name Linux and the BSDs only.                                               | `nyanpasu_ipc/src/utils/mod.rs:5-21`; `nyanpasu_ipc/src/utils/os.rs`; `nyanpasu_ipc/src/server/mod.rs:8,29-38` |
| Service installation | `nyanpasu-service-runtime` installs through `service-manager` (systemd, launchd, Windows SCM).                                                                                                                                             | `nyanpasu-service-runtime/Cargo.toml:66`                                                                       |
| Frontend platform    | Components use OS flags for both presentation and native-only actions; a single platform snapshot is proposed in #6257 (draft).                                                                                                            | #6257                                                                                                          |

Paths starting with `nyanpasu_ipc/`, `nyanpasu-core-manager/` and
`nyanpasu-service-runtime/` are inside the `backend/nyanpasu-runtime` submodule
(`nyanpasu-core-manager` and `nyanpasu-service-runtime` under `crates/`).
Changes there are PRs in
[libnyanpasu/nyanpasu-runtime](https://github.com/libnyanpasu/nyanpasu-runtime)
followed by a gitlink update on a released tag.

## 3. Package ownership

Working names; the PR that creates each crate settles its name.

| Package                         | Role                                                                                                          | Depends on                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Mobile shell (`backend/mobile`) | Tauri mobile app: composition root, Tauri IPC binding, mobile presentation adapters, Android and iOS projects | `nyanpasu-rpc`, core, implementation crates, its target crates |
| `nyanpasu-android`              | Target crate: root detection, `su`-based `ServiceProcessControl` and `ElevationConsent`, module install       | Foundation layer only                                          |
| `nyanpasu-ios`                  | Target crate: jailbreak/TrollStore detection, daemon control and consent                                      | Foundation layer only                                          |
| Android and iOS plugins         | Kotlin `VpnService` and Swift NetworkExtension code, as Tauri mobile plugins owned by the shell               | The shell                                                      |

- The mobile shell is a separate shell, not `backend/tauri` with `cfg(mobile)`.
  Desktop presentation (tray, widget, Slint windows, global shortcuts) and
  `nyanpasu-os` stay out of the mobile dependency graph instead of being gated
  inside one crate. Whether the two Tauri shells share their Tauri IPC binding
  through a small crate is open question O-1.
- The mobile shell provides no window control, hotkey validation or
  presentation effect executor at first. Core already reports these as
  structured unsupported (phase C1b), and the E1 capability admission rejects
  commands the shell cannot serve with `unsupported`.
- The target crates follow the [backend package rules](../../development/backend-packages.md):
  foundation layer only, no dependency on core or other implementation crates,
  registered in `lint:backend-boundaries`, the static gate prefixes and the musl
  runner where applicable.

## 4. Mode selection

- The mode is the existing service-mode setting. Root mode corresponds to
  service mode on; the unprivileged mode is service mode off.
- The two modes never capture traffic at the same time. Switching to root mode
  stops the `VpnService` or NetworkExtension tunnel before the daemon starts
  capture; switching back stops the daemon's capture first.
- On a device without root (or without a jailbreak), the shell injects a
  refusing `ElevationConsent`, as the planned headless server does. Enabling
  service mode then fails before any `su` call with the existing structured
  refusal.

## 5. Android root mode

### 5.1 Daemon host

- `nyanpasu-service` is built for `aarch64-linux-android` (and
  `x86_64-linux-android` for emulators). On Android it does not use
  `service-manager`; it runs in the foreground under the module's `service.sh`,
  which the root manager starts at boot (late-start service stage). Boot
  autostart therefore belongs to the module, not to the service runtime.
- The module directory holds the daemon, the core binaries and the daemon's
  state. The core keeps running as a child of the daemon, as on desktop.
- `install`, `uninstall` and `update` map to installing, removing and
  replacing the module zip through the detected manager CLI
  (`ksud module install`, `magisk --install-module`, `apd module install`).
  Install and uninstall take effect only after a reboot under all three
  managers; the shell reports that as a pending state, not as a failure. Open
  question O-2 is whether the first start may happen without a reboot by
  launching `service.sh` once through `su`.
- `start_daemon`, `stop_daemon` and `probe` call the daemon's own commands
  through `su -c`, and `probe` keeps its contract: real output or an error,
  never a synthesized "stopped".

### 5.2 IPC endpoint and peer authorization

The desktop endpoint policy does not carry over: `/var/run` does not exist on
Android, there is no `nyanpasu` group, and each app has its own UID.

- The endpoint becomes a per-platform policy in `nyanpasu-ipc`: name, creation
  and peer authorization, with the desktop behavior unchanged.
- **Proposed Android endpoint:** a Linux abstract-namespace socket, which needs
  no directory that both the root domain and the app can reach. Abstract
  sockets have no file permissions, so the daemon authorizes each connection by
  `SO_PEERCRED`: the peer UID must equal the app UID recorded at install time.
  The alternative is a path socket in a directory the module creates with the
  app's UID as owner (O-3).
- SELinux decides whether the app domain (`untrusted_app`) may connect to a
  socket created by the root manager's domain. The module ships the needed
  allow rules in `sepolicy.rule`; the exact rules and their differences between
  KernelSU, Magisk and APatch are verified in the spike (tasks M1).

### 5.3 Traffic capture

- The daemon has root, so mihomo opens `/dev/tun` itself and uses its own
  routing. Per-app selection uses mihomo's Android TUN filters
  (`include-package`, `exclude-package`, `include-android-user`).
- Transparent proxy with iptables owner matching is the alternative capture
  mode. It should reuse the Linux tproxy work in #6258 instead of a separate
  Android implementation.
- Interaction with Android Private DNS and with other VPN apps is recorded as a
  known limitation, not handled in the first version.

### 5.4 Elevation and consent

- `ElevationConsent` on a rooted device permits `ElevationOperation::Service`;
  the root manager's grant dialog is the "existing OS prompt". A denied grant is
  an execution failure of the command, as a cancelled desktop prompt is.
- `DnsFlush` and `CoreBinaryCopy` are refused on Android: there is no DNS cache
  to flush from the app, and core binaries are updated inside the module by the
  daemon's update command.

## 6. iOS privileged mode

- **Rootless jailbreak (for example Dopamine, palera1n):** the daemon ships as a
  `.deb` with a LaunchDaemon plist under `/var/jb/Library/LaunchDaemons` and
  runs as root. The XNU `utun` path that mihomo uses on macOS is expected to
  work unchanged (to verify, M8). The endpoint is a Unix socket under the
  jailbreak prefix with peer-UID authorization.
- **TrollStore:** apps installed with TrollStore can carry the entitlements to
  spawn a root helper. The helper replaces the LaunchDaemon; it does not
  survive reboots, so the shell starts it on launch.
- Whether a sandboxed mobile shell may connect to the daemon's socket at all,
  or must itself be installed unsandboxed, is open question O-4.
- `ServiceProcessControl` maps to `launchctl` (jailbreak) or the helper's own
  lifecycle (TrollStore); `install` and `uninstall` are package-manager
  operations the user performs, so the shell reports them as unsupported.

## 7. Unprivileged modes (outline)

These need their own refined spec before implementation; this section records
the constraints that affect package design now.

- **Android `VpnService`:** a Kotlin plugin establishes the VPN, excludes the
  app's own package, and hands the TUN file descriptor to the core
  (`tun.file-descriptor`). Android permits executing binaries packaged in the
  app's native library directory, so the core may stay a child process started
  by `LocalEndpoint`, given a fd-passing hook in the core manager (O-5).
- **iOS NetworkExtension:** the extension is a separate process that cannot
  spawn children and has a tight memory limit. The core must run in process
  inside the extension, and the app talks to it across the app/extension
  boundary. This is a third `ControlEndpoint` and the largest piece of mobile
  work.

## 8. Prerequisites and ordering

| Prerequisite                       | Needed by              | Why                                                                                                    |
| ---------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------ |
| D1–D4 (#6224, #6229, #6231, #6232) | Mobile shell           | They restructure the state engine, mutation participation and public runtime values a shell builds on. |
| A2b (#6234)                        | Mobile shell           | Script engines become selectable; a mobile build may need to omit one.                                 |
| E1, E2 (#6235, #6236)              | Mobile shell           | Shared commands and the registry leave the Tauri crate; capability admission reports `unsupported`.    |
| #6257 (frontend platform snapshot) | Mobile UI              | Mobile presentation and native-only actions need one platform source.                                  |
| E3–E5 (#6239, #6240, #6241)        | Not needed             | Local socket, server and CLI; mobile runs core in process.                                             |
| #6258 (Linux tproxy)               | Android tproxy capture | Optional capture mode only.                                                                            |

The runtime work (§5.1, §5.2) and module packaging depend on none of these and
may start immediately.

## 9. Open questions

- **O-1:** Share the Tauri IPC binding between the desktop and mobile shells
  through a small crate, or keep one copy per shell? The architecture rule "no
  desktop interaction crate" does not cover a transport binding.
- **O-2:** Allow the first start after module install without a reboot?
- **O-3:** Abstract socket with `SO_PEERCRED` or a path socket in an
  app-owned directory?
- **O-4:** Can a sandboxed iOS app reach the jailbreak daemon's socket?
- **O-5:** Child process with a passed TUN fd, or an in-process core library,
  for Android `VpnService` mode?
- **O-6:** Distribution: GitHub releases and F-Droid for Android; whether a
  Play Store build without root mode is in scope.

## 10. Out of scope

- Mobile UI layout and navigation, which get their own spec after #6257.
- Play Store and App Store submission.
- OpenWrt and the headless server (E4, E5).
- Changing desktop service mode behavior.
