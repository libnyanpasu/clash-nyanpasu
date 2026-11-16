# Network-aware proxy automation

**Date:** 2026-11-15

**Status:** Implemented in one change (system proxy, TUN and transparent proxy together). Section 10 lists what the first version does not do.

**Baseline:** `main@2486f693f`.

**Authority order:** current `AGENTS.md` and the development guides > this spec.

## 1. Summary

A user wants capture to follow the network the machine is on: off at home where the router already proxies, on elsewhere, or the reverse. Each capture mechanism was a manual switch.

This spec adds **network rules**: an ordered list of named networks, each with an action per mechanism (system proxy, TUN, Linux transparent proxy). When the machine arrives on a network, the first matching rule **directly configures** each mechanism: `enable`, `disable`, or `keep`.

```text
arrive on network → first enabled matching rule (else default actions) → write the existing switches
```

Decision record (maintainer, before implementation):

| Decision                                                        | Result                                                                                                                          |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Do rules suppress existing enabled switches, or configure them? | **Configure them.** A rule can turn a mechanism on as well as off. An earlier draft proposed a suppress-only overlay; rejected. |
| Which mechanisms ship?                                          | **System proxy, TUN and transparent proxy together.**                                                                           |

Because rules write the ordinary switches (`enable_system_proxy`, `enable_tun_mode`, `transparent_proxy.mode`), there is no second state to project into the effects owner or the runtime build. A rule's write is an ordinary configuration mutation: it validates, commits, builds and tries the runtime when TUN changes, applies post-commit effects, and degrades the way the same edit from the settings page does. Tray, widgets, hotkeys and the dashboard need no change: they already show the switches.

## 2. Existing capabilities used

| Fact                                                                                                                                       | Location                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| System proxy intent is `NyanpasuAppConfig.enable_system_proxy`; patching it goes through `ApplicationClient`.                              | `nyanpasu-config/src/application/mod.rs`, `nyanpasu-core/src/client/application.rs`   |
| TUN is `ClashConfig.enable_tun_mode`; transparent capture is `ClashConfig.transparent_proxy.mode`. Both patch through `ClashConfigClient`. | `nyanpasu-config/src/clash/config/mod.rs`, `nyanpasu-core/src/client/clash_config.rs` |
| TUN and transparent capture exclude each other; transparent capture needs the selected core, Linux and service mode.                       | `ClashConfig::validate_transparent_proxy`, `NyanpasuClient::patch_clash_config`       |
| Transparent capture availability is the service host's report.                                                                             | `nyanpasu-core/src/client/transparent_proxy.rs`                                       |
| `patch_app_config` accepts ordinary preferences and is `rpc(http)`.                                                                        | `nyanpasu-rpc/src/commands/runtime.rs`, `docs/development/rpc.md`                     |

What did not exist: any observation of the network. `nyanpasu-os::network_environment` is new OS integration.

## 3. Behavior

### 3.1 Observation

The observed value is the **primary network**: the up, non-loopback, non-tunnel interface that has a link-layer address and a default gateway, preferring the one the default route uses.

```rust
pub enum NetworkState {
    Offline,                              // no such interface
    Online { network: NetworkObservation },
}

pub struct NetworkObservation {
    pub interface_kind: InterfaceKind,    // Wifi | Ethernet | Other
    pub wifi: WifiName,                   // NotWifi | NotRead | Known { ssid } | Unavailable { reason }
    pub gateway_mac: Option<String>,      // lowercase aa:bb:cc:dd:ee:ff
}
```

- **No feedback loop.** A TUN device has no link-layer address and is filtered by kind, so enabling TUN does not change the observation (tested on the filter, `nyanpasu-os::network_environment`).
- **Settling.** The actor reads the network every 5 s while the policy is enabled. A network is acted on when two consecutive readings agree, so a burst of changes inside one period does not write the switches.
- **Offline holds.** While offline nothing is written; the switches stay as they are. Returning to the same network writes nothing; arriving on a different match writes.
- **Startup.** The first reading is taken as the actor starts, and rules apply about one period (5 s) later, after the second agreeing reading. Capture state during those seconds is whatever the persisted switches say.
- **Wi-Fi name.** See 3.1.1.
- **Change notifications.** None are used; polling is the whole mechanism (section 10).

#### 3.1.1 The Wi-Fi name and location consent

The name is read only for a policy that needs it: the policy is enabled and an enabled rule has a Wi-Fi name condition (`NetworkAutomation::reads_wifi_name`). Otherwise the port is called with `WifiNameRead::Skip` and a Wi-Fi network reads as `NotRead`. This matters because macOS and Windows gate the name behind location access: reading it is what shows the system prompt and the location-in-use indicator, so a user who never writes a name rule is never asked.

| Platform          | API                                                                   | Consent                                                                                                                                                                                                                                              |
| ----------------- | --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux             | NetworkManager over D-Bus (`ActiveAccessPoint`)                       | None. Hosts with iwd or wpa_supplicant only report `Unreadable`.                                                                                                                                                                                     |
| macOS             | CoreWLAN `CWWiFiClient.interfaceWithName(_:).ssid()`                  | `ssid()` answers nothing unless Location Services is on and the app is authorized. The adapter reads `CLLocationManager.authorizationStatus` first: authorized reads, undecided is `PermissionRequired`, denied or restricted is `PermissionDenied`. |
| Windows 11 24H2 + | `WlanQueryInterface(wlan_intf_opcode_current_connection)` (`wlanapi`) | The call returns `ERROR_ACCESS_DENIED` without location access; the first such call shows a one-time system prompt. That is `PermissionDenied`: Windows does not say whether the prompt was refused or never shown.                                  |
| Other             | -                                                                     | `Unsupported`.                                                                                                                                                                                                                                       |

`WifiName::Unavailable { reason }` carries `PermissionRequired` (asking can show a prompt), `PermissionDenied` (only system settings can change it), `Unreadable` (not associated, or the name service failed) or `Unsupported`. A Wi-Fi name condition holds only for `Known`, so every other state falls through to the next rule or the default, never to a guess.

- **Asking.** A poll never asks. `request_wifi_name_access` (desktop mutation, `WifiNameRead::ReadAndAsk`) reads the name once on the user's behalf; on macOS it also starts `requestWhenInUseAuthorization` on the main queue, once per process. On Windows a reading is itself the request. The rule editor's "Use current network" calls it when the status holds no name, and the status panel shows an **Allow** button for `PermissionRequired` and **Open settings** (`x-apple.systempreferences:` / `ms-settings:privacy-location`, through the existing opener) for `PermissionDenied`.
- **Throttle.** The adapter reuses a name read for the same interface and gateway MAC for 30 s on macOS and Windows, so the 5 s poll does not keep the system's location indicator lit. A different SSID behind the same gateway is therefore noticed up to 30 s late. An explicit request always reads.
- **Policy edits.** When an edit changes whether the policy reads the name, the actor discards its readings and settles again; an old reading without the name would otherwise be evaluated against the new rule and fall through to the default.

### 3.2 Rules and evaluation

```rust
pub struct NetworkAutomation {
    pub enabled: bool,                     // default false
    pub default_actions: MechanismActions, // networks no rule matches; default all Keep
    pub rules: Vec<NetworkRule>,           // ordered
}
pub struct NetworkRule { id, name, enabled, conditions: NetworkConditions, actions: MechanismActions }
pub struct NetworkConditions { wifi_ssid, gateway_mac, interface_kind }   // all present fields must hold
pub enum MechanismAction { Keep, Enable, Disable }
```

`NetworkAutomation::evaluate(&observation)` is a pure function: the first **enabled** rule whose conditions all hold decides all three mechanisms; otherwise `default_actions`. A condition over a fact the observation lacks (SSID when `Unavailable`, gateway MAC when unresolved) never holds, so an unreadable network can only fall through to the default actions.

SSID comparison is exact; gateway MAC comparison ignores case.

Validation, in `nyanpasu-config`, before commit: at most 64 rules; non-empty unique ids; non-empty names; at least one condition per rule; non-empty SSID; gateway MAC as six hex octets and not the zero address; no action set that enables both TUN and transparent proxy (rule or default).

### 3.3 When a rule writes

The actor remembers the last evaluation it applied. It writes when:

- the settled network evaluates to a **different** answer (a different matching rule, or different actions) than the last applied one;
- the policy is edited so the settled network's answer changes (editing an unrelated rule's name writes nothing);
- the user presses **Apply now**, which re-reads the network without waiting to settle and writes even when the answer is unchanged.

A manual switch change therefore **stands until the machine arrives on a network with a different answer**. This is the intended precedence: rules set the switches on arrival; the user stays in control in between. Disabling the policy forgets what was applied, so enabling it again applies to the current network.

### 3.4 Writing the switches

`ConfigCaptureSwitches` (core) applies one `MechanismActions` through the same owners a settings change uses and returns an outcome per mechanism: `unchanged`, `enabled`, `disabled`, or `failed { message }`.

- **System proxy:** one `ApplicationClient` patch of `enable_system_proxy`, skipped when already at the requested value.
- **TUN and transparent proxy:** one `ClashConfigClient::patch_validated` patch, so the exclusivity, core and service-mode validation of a settings edit applies. Enabling one turns the other off, even when the other's action is `keep`. Transparent proxy, when requested on, is first checked against the service's `transparent_proxy_status`; if the host cannot do it, that mechanism reports `failed` and **TUN stays as its own action left it** (nothing replaces it).
- A refused patch (validation, runtime candidate check, a service-mode requirement) reports `failed` for the mechanisms it carried. The other mechanisms are still written.
- A committed write whose side effect degraded (for example, an OS proxy write that failed) is logged at warn level with the degradation; the switch value stands, as it does for a settings change, and the configuration status surfaces the degradation.

## 4. Failures

| Failure                       | Behavior                                                                                                                                                                     |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host has no network observer  | `nyanpasu-server` composes none. Status is `supported: false`. Saving an **enabled** policy is rejected before commit (`InvalidNetworkAutomation`); a disabled policy saves. |
| Policy invalid                | Rejected before commit by `NetworkAutomation::validate`, surfaced as `InvalidNetworkAutomation { reason }`.                                                                  |
| Network cannot be read        | `Offline`. Nothing is written.                                                                                                                                               |
| Wi-Fi name unavailable        | `WifiName::Unavailable { reason }`, shown in the status panel with what to do (3.1.1); SSID conditions do not hold. Other conditions work.                                   |
| A switch write fails          | That mechanism reports `failed` in the status; the others still apply. The next different network, or Apply now, tries again.                                                |
| Transparent proxy unsupported | `failed` for that mechanism only; TUN is not turned off in its place.                                                                                                        |
| Actor stopped                 | Status calls return `supported: false` rather than panicking.                                                                                                                |
| Shutdown                      | The actor stops polling and writing once the root token is cancelled; a started write finishes.                                                                              |

## 5. Architecture

| Layer                     | Item                                                                                                                                   |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `nyanpasu-config`         | `application/network_automation.rs`: policy, observation values, validation, pure `evaluate` (pure service)                            |
| `nyanpasu-contracts`      | `NetworkEnvironmentPort` (blocking `current(WifiNameRead) -> NetworkState`), beside `OsProxyPort`                                      |
| `nyanpasu-os`             | `network_environment` (feature `network-environment`): `NetdevNetworkEnvironment`; per-OS Wi-Fi name readers in `wifi/`                |
| `nyanpasu-runtime-values` | `NetworkAutomationStatus`, per-mechanism outcomes: public wire values                                                                  |
| `nyanpasu-core`           | `client/network_automation/`: `NetworkAutomationActor` + `NetworkAutomationClient`, `CaptureSwitches` port and `ConfigCaptureSwitches` |
| `nyanpasu-rpc`            | `get_network_automation_status`, `apply_network_automation`, `request_wifi_name_access` (thin)                                         |
| `backend/tauri`           | Composition only: injects `NetdevNetworkEnvironment` through `ClientHostAdapters.network_environment`                                  |

- **Actor** because it owns long-lived state (the last reading, the settled network, what was last applied), a poll timer and a subscription to settings changes; one mailbox serialises a poll, a policy edit and Apply now so they cannot interleave while switches are written.
- **Pure service:** `validate` and `evaluate`. Plain-value tests, no IO.
- **Port:** `NetworkEnvironmentPort` is optional (`ClientHostAdapters.network_environment: Option<Arc<dyn ...>>`), so a shell without it is `unsupported`. The blocking read runs on the blocking pool.
- The actor receives the committed policy by watching the application settings; it does not read or notify any sibling owner. Its writes go through the existing owners. No new global, no Tauri type, no `AppHandle`.
- `NyanpasuClient::spawn_network_automation` is the only wiring; the facade exposes `get_network_automation_status` and `apply_network_automation`, never the actor.
- New dependencies (`netdev`, `zbus`) are optional features of `nyanpasu-os` only and checked by `pnpm run lint:backend-boundaries`.
- Moving the transparent-proxy switch patch and status read from `hotkey/` and `NyanpasuClient` to `client/transparent_proxy.rs`, and the validated clash patch to `ClashConfigClient::patch_validated`, was required so the hotkey and the rules share one implementation instead of copying it.

## 6. Persistence and RPC

- Field `network_automation` on `NyanpasuAppConfig`, `#[serde(default)]`, disabled and empty by default. Installations without the key decode unchanged; a test removes the key from a saved document. The patch replaces the whole policy, preserving rule order.
- It was specified as an ordinary preference, written through `patch_app_config`. With the remote-access scopes, those switches need the `system` scope, so the policy is written through `SystemSettingsPatch` and `patch_system_settings` instead: a `settings` principal must not gain them through a rule. `get_network_automation_status` is `sensitive_read` and `apply_network_automation` and `request_wifi_name_access` are `system`. Consequence: `get_app_config` (HTTP) returns the saved rules, including Wi-Fi names and gateway MACs, and configuration backups contain them.

| Command                         | Kind     | Targets | Notes                                                                                                             |
| ------------------------------- | -------- | ------- | ----------------------------------------------------------------------------------------------------------------- |
| `get_network_automation_status` | query    | desktop | Live network and last application. Desktop-only because it carries the live Wi-Fi name and gateway.               |
| `apply_network_automation`      | mutation | desktop | Re-applies to the current network. Optional capabilities `os_integration`, `presentation`, `elevation`.           |
| `request_wifi_name_access`      | mutation | desktop | Reads the Wi-Fi name once, asking the OS for location access where needed. Returns a status holding that reading. |

No `rpc(http)` and no `rpc(local)`; the allowlist of thirteen local operations is unchanged. The frontend polls the status every 5 s; there is no new shared event (the host has no network-change event, and the status is cheap). Bindings, command catalog and capability inventory are regenerated.

## 7. Frontend

Settings → System → Proxy gains a **Network Rules** group (desktop surface only; the browser surface does not render it and issues no desktop-only command):

- Switch "Follow Network Rules".
- When enabled: the current network and the matched rule, an **Apply now** button, the ordered rule list (enable switch, edit, delete), **Add rule**, and the default actions for other networks.
- The rule dialog takes a name, Wi-Fi name, gateway MAC, connection type and one action select per mechanism (transparent proxy on Linux only), with **Use current network** pre-filling from the live observation (asking for the Wi-Fi name first when nothing has read it, 3.1.1), and a note that a Wi-Fi name alone can be spoofed.
- A Wi-Fi name that is unavailable shows a notice, in the status panel and in the dialog after **Use current network**, with the reason and **Allow** or **Open settings**.

Text lives under `settings_system_proxy_network_automation_*` (the Wi-Fi notices under `_wifi_*`) in all six locales, plus `error_config_invalid_network_automation`.

## 8. Security and privacy

- **A network name is not an authenticator.** Anyone can broadcast a trusted SSID; a rule on the SSID alone lets a nearby attacker choose your switch values. The rule dialog says so and offers the gateway MAC as a second condition, which raises the cost but is also forgeable on the same link.
- **Fail toward the default.** An unreadable or unnamed network falls to the default actions, which are `keep` unless the user changed them.
- **Location access is requested only for a name rule.** The Wi-Fi name is not read, and no consent is asked, until an enabled rule matches on it or the user presses **Use current network** (3.1.1). The application does not read or send the device's location; the name is gated behind location access because Wi-Fi names can reveal where a device is. `backend/tauri/Info.plist` carries the macOS usage description.
- **Identifiers are sensitive.** SSID and gateway MAC are never logged by the actor (it logs only the match kind or rule id). They are stored in plain text in the application config (section 6). Diagnostic-archive redaction of the saved rules is **not** implemented in this change (section 10).
- **No new privilege.** Observation is unprivileged and in the user's process; transparent capture still goes through the typed service IPC.
- **Remote control.** An HTTP caller can already change the switches; with this change it can also edit the rules through `patch_app_config`. It cannot read the live network (status is desktop-only).

## 9. Tests

- **Pure (config):** first-match order, disabled rule skipped, default on no match, every condition and missing-fact never holds, validation variants (by variant, not `is_err`), complete policy keeps order, absent key decodes to default, patch replaces the policy.
- **Actor (typed client, fake `NetworkEnvironmentPort`, fake `CaptureSwitches`, no sleeps, polls delivered by message):** needs two agreeing polls; unstable readings write nothing; each arrival on a different match writes once; offline holds; policy edit writes only a changed answer; disabled policy observes but does not write; Apply now re-writes and does not wait to settle; stopped actor reports unsupported.
- **Switches (real config owners, test clients):** system proxy written once and idempotent; enabling TUN turns a kept transparent proxy off; unsupported transparent proxy fails alone and keeps TUN.
- **Facade:** host without observer rejects an enabled policy, saves a disabled one, rejects an invalid policy with the reason.
- **Adapter (`nyanpasu-os`):** primary-link selection; a link-layer-less (tunnel) interface is excluded; the name cache (hit within the TTL, miss for another interface or gateway, zero TTL never hits); Windows adapter-name GUID parsing; macOS authorization-status mapping (compiled and run on macOS only).
- **Actor, Wi-Fi name:** polls skip the name unless a name rule exists; a policy that starts matching on the name settles before applying; an explicit request asks, returns the reading and leaves the settled network alone; a refused name matches no name rule.
- **RPC:** command catalog and capability inventory freshness; the error receiving fixtures include the new variant.
- **Frontend (Vitest browser):** deleting a rule saves the remaining rules in order; a rule created from the current network keeps its identifiers and is appended; **Use current network** asks for a name nobody has read and uses the answer; a refused permission is explained without asking again; **Allow** and **Open settings** (macOS and Windows URLs) reach their callbacks; a known or unwanted name shows no notice.

Not covered by automated tests: real NetworkManager, the CoreWLAN, Core Location and WLAN API calls themselves, the system prompts, real network changes, sleep/wake. The macOS and Windows adapters are type-checked and linted for their targets and compiled again by those OS CI jobs. Platform acceptance remains manual (section 10).

## 10. Limitations and deferred work

- **No change notifications.** Polling every 5 s while enabled; a change takes up to two periods to act on. Netlink, `SCDynamicStore` and `NotifyIpInterfaceChange` subscriptions are the follow-up.
- **Wi-Fi name adapters for macOS and Windows have not run on a device.** They follow the platform documentation (3.1.1) but the prompts, the denied and undecided states and the settings links are unverified until the manual acceptance below. Known platform limits: macOS offers the location prompt only to a bundled app (a development build stays undecided), and a signed and notarized build must also carry the hardened-runtime location entitlement (`com.apple.security.personal-information.location`), which the release workflow does not set because it does not sign with an Apple identity; Windows 11 before 24H2 does not gate the name, and 24H2 and later cannot report refused versus never shown. Developers report CoreWLAN returning no name even with Location Services enabled on some macOS 14 and 15 releases; that reads as `Unreadable`. Linux without NetworkManager has no name.
- **Startup delay.** Rules apply about 5 s after launch, after the runtime has started with the persisted switches. A start-before-runtime read is not implemented.
- **Manual switches stand** until the next different match (3.3); there is no "pause rules" control.
- **Not implemented:** per-network profile, mode or core control; open-network warning; diagnostic redaction of rules and observation; IPv4 subnet and BSSID conditions; UI reorder (rules are appended and edited; order can only be changed by delete and re-add); mobile; `nyanpasu-server` (unsupported).
- **Platform acceptance still to do by hand**, per OS: Wi-Fi A → Wi-Fi B → Ethernet → offline → Wi-Fi A with and without TUN active; sleep and wake; Linux with NetworkManager, with iwd only and without Wi-Fi. On macOS (bundled app): a name rule with permission undecided shows **Allow**, the prompt appears, allowing makes the rule match, denying then shows **Open settings** and the Location Services pane opens. On Windows 11 24H2 or later: with location off for desktop apps the first name rule triggers the system prompt, refusing shows **Open settings**, and enabling location makes the rule match without a restart; on Windows 10 and Windows 11 before 24H2, the name reads without a prompt.
- **Downgrade:** an older release does not know `network_automation`; whether it drops the key when it rewrites the config was not verified.
