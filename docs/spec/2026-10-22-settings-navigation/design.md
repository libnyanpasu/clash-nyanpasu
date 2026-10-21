# Settings navigation redesign: M3E sidebar and page split

**Date:** 2026-10-22
**Baseline:** `feat/material-expressive-settings @ 5bad1d4bd`
**Status:** Agreed (§2.1). Implemented in the phases of §8.

## 1. Background and problems

The settings screen has a flat sidebar of seven items
(`settings/_modules/settings-navigate.tsx`) and one page per item. As features
have been added, these problems have appeared:

- **The Clash Settings page concentrates too much.** It has six sections, about
  20 setting rows, and the core switch / update / restart actions on one page.
  Core behavior (unified-delay and others), inbound settings (ports, LAN, IPv6),
  how Nyanpasu talks to the core (control channel), core management actions,
  profile conversion rules (field filter), and latency tests all live together.
  The page is long enough that it defers rendering its lower sections with
  `useDeferredValue`.
- **The Nyanpasu Settings page has the same problem.** It has seven sections:
  logs, traffic history, the network statistic widget, Enhance, tray, window
  close behavior, and keyboard shortcuts. Its sidebar description is only
  "Nyanpasu specific settings", which says nothing about its content.
- **Related settings are split across pages.** The control channel (IPC / HTTP,
  disabling the HTTP controller) is on Clash Settings, while the HTTP external
  controller address, port strategy, and secret are on "Clash External Control".
  Both describe how Nyanpasu controls the core, and they affect each other.
- **The sidebar has no hierarchy.** Seven items of equal weight; new items only
  make the list longer.

## 2. Goals and non-goals

Goals:

1. **Phase 1:** Build a Material 3 Expressive (M3E) sidebar modeled on the
   Android 17 Settings screen. Items are grouped, and each group sits in its own
   rounded container.
2. **Phase 2:** Reclassify the settings and split the pages that concentrate
   them into pages of a suitable size. The goal is maintainability,
   extensibility, and findability, not more pages.

Non-goals:

- Do not change setting behavior, the backend, RPC, or the config schema.
  Existing setting components are moved, not rewritten.
- Do not redesign the rows inside setting pages (`SettingsCard`,
  `SettingsGroup`, and the other existing components).
- Settings search is not part of Phases 1 and 2. It follows in a later phase
  (§8), designed on top of the split pages.

### 2.1 Decisions

1. Four groups and 16 items, as in §4.2. Widget stays its own item.
2. Updates becomes its own item in the Nyanpasu group, split out of About.
3. The open source licenses page becomes its own item, "Licenses", in the About group, next to the
   About page.
4. Debug Tools is the last item of the Nyanpasu group.
5. The third group is named "Nyanpasu".
6. Web UI merges into Control Channel.
7. The core log level moves to Logs & History.
8. System splits into two pages: Proxy, and Service & Startup.
9. Each item has its own icon color (§5.3), not one color per group.
10. Old URLs are not kept: no redirects. In-app links move to the new paths.
11. The About page keeps the help links (wiki, issues, GitHub) next to the
    version.
12. The search bar is implemented in a later phase.

## 3. Current inventory

| Current page (path)                | Section                    | Content (component → setting key)                                                                                                                                                                                                                                                      |
| ---------------------------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| System Settings (`/system`)        | Routing Mode               | `SystemProxyButton` / `TunModeButton`, `ProxyModeSelector` (rule / global / direct)                                                                                                                                                                                                    |
|                                    | Proxy Guard                | `ProxyGuardSwitch` (`enable_proxy_guard`), `ProxyGuardConfig` (`proxy_guard_interval`), `ProxyBypassConfig` (`system_proxy_bypass`)                                                                                                                                                    |
|                                    | Current System Proxy       | `CurrentSystemProxy` (read-only)                                                                                                                                                                                                                                                       |
|                                    | System Service             | `SystemServiceSwitch` (`enable_service_mode`), `SystemServiceCtrl` (install, start, stop and more; 577 lines)                                                                                                                                                                          |
|                                    | Startup Settings           | `AutoLaunchSwitch` (`enable_auto_launch`), `SilentLaunchSwitch` (`enable_silent_start`)                                                                                                                                                                                                |
|                                    | System Tools (Win / macOS) | `DnsCacheButton`, `UwpToolsButton` (Windows only)                                                                                                                                                                                                                                      |
| User Interface (`/user-interface`) | Language / Theme / CSS     | `LanguageSelector`, `ThemeModeSelector`, `ThemeColorConfig`, `CustomCssCard`                                                                                                                                                                                                           |
| Clash Settings (`/clash`)          | Clash Settings             | `AllowLanSwitch`, `IPv6Switch` (Clash config), `ManagedGuardFieldSelector` (unified-delay / tcp-concurrent, `overrides`), `TunStackSelector` (`tun_stack`), `LogLevelSelector` (core log level), `CoreLogStorageConfig` (`core_logs`), `ExpandIncludeAllSwitch` (`expand_include_all`) |
|                                    | Port Settings              | `MixedPortConfig`, `OptionalPortConfig` (socks / http)                                                                                                                                                                                                                                 |
|                                    | Control Channel            | `ControlChannelSettings` (`clash_control_channel`, `clash_ipc_disable_http_controller`)                                                                                                                                                                                                |
|                                    | Core Manager               | `CoreManagerCard` (switch `core`, update the core, restart the sidecar, fetch remote versions)                                                                                                                                                                                         |
|                                    | Clash Field Filter         | `FieldFilterSwitch` (`enable_clash_fields`), `FieldFilterCard`                                                                                                                                                                                                                         |
|                                    | Latency Test               | `LatencyTestConfig` (`default_latency_test`, `default_latency_timeout_ms`)                                                                                                                                                                                                             |
| Clash External Control (`/web-ui`) | External Control           | `ExternalControllerConfig`, `PortStrategySelector` (`external_controller`), `CoreSecretConfig`                                                                                                                                                                                         |
|                                    | Web UI                     | `WebUI` (`web_ui_list`)                                                                                                                                                                                                                                                                |
| Nyanpasu Settings (`/nyanpasu`)    | Log Settings               | `LogLevelSelector` (`app_log_level`), `LogFileConfig` (`max_log_files`, `max_log_file_size`)                                                                                                                                                                                           |
|                                    | Traffic History            | `TrafficRetentionSelector` (`traffic_retention`), `LocalIpProbeSwitch` (`enable_local_ip_probe`)                                                                                                                                                                                       |
|                                    | Network Statistic Widget   | `NetworkStatisticWidgetSelector` and three more (`network_statistic_widget`, `network_statistic_widget_logo`)                                                                                                                                                                          |
|                                    | Enhance                    | `BreakWhen{Proxy,Profile,Mode}Change*` (`break_connection`), `EnableBuiltinEnhancedSwitch` (`enable_builtin_enhanced`)                                                                                                                                                                 |
|                                    | System Tray                | `TrayMenuModeSelector` (`tray_menu_mode`), `TrayProxiesSelector` (`tray_selector_mode`), `TrayIconConfig` (not on macOS)                                                                                                                                                               |
|                                    | Close Windows              | `WindowCloseBehaviorCards` (`window_close`)                                                                                                                                                                                                                                            |
|                                    | Keyboard Shortcuts         | `HotkeyManager`                                                                                                                                                                                                                                                                        |
| Debug Tools (`/debug`)             | —                          | Path utilities, configuration status, Advance Tools                                                                                                                                                                                                                                    |
| About (`/about`, `/about/credits`) | —                          | Version, update controls, update sources, update preferences, open source credits                                                                                                                                                                                                      |

## 4. Information architecture: groups and items

### 4.1 Classification principles

- **Group by what a setting affects:** OS integration, the core (Clash), the
  app itself (Nyanpasu), and information about the app — four groups.
- **One page, one task.** A user who opens a page for one purpose should finish
  there. Settings that affect each other (such as the control channel and the
  HTTP controller) share a page.
- **Do not over-split.** No page with only one or two rows. Aim for two to four
  sections per page.

### 4.2 Structure (4 groups, 16 items)

```text
System
  ├─ Proxy                 /main/settings/system/proxy
  └─ Service & Startup     /main/settings/system/service
Clash
  ├─ Core Manager          /main/settings/clash/core-manager
  ├─ Core Settings         /main/settings/clash/core
  ├─ Ports & Network       /main/settings/clash/network
  ├─ Control Channel       /main/settings/clash/control
  └─ Runtime Config        /main/settings/clash/runtime
Nyanpasu
  ├─ User Interface        /main/settings/nyanpasu/interface
  ├─ Tray & Windows        /main/settings/nyanpasu/tray
  ├─ Widget                /main/settings/nyanpasu/widget
  ├─ Keyboard Shortcuts    /main/settings/nyanpasu/shortcuts
  ├─ Logs & History        /main/settings/nyanpasu/logs
  ├─ Updates               /main/settings/nyanpasu/updates
  └─ Debug Tools           /main/settings/debug (unchanged path)
About
  ├─ Clash Nyanpasu        /main/settings/about (unchanged path)
  └─ Licenses  /main/settings/about/credits (unchanged path)
```

Page contents:

| Group    | Item               | Sections and content                                                                                                                              | Moved from                |
| -------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| System   | Proxy              | Routing mode (system proxy / TUN, rule / global / direct), proxy guard (guard, interval, bypass), current system proxy                            | System                    |
|          | Service & Startup  | System service (service mode, service control), startup (start on login, start in background), system tools (DNS cache, UWP)                      | System                    |
| Clash    | Core Manager       | `CoreManagerCard` (switch, update, reinstall the core, restart the sidecar)                                                                       | Clash                     |
|          | Core Settings      | Core behavior (unified-delay, tcp-concurrent), latency test (URL, timeout), connections (close connections when switching proxy / profile / mode) | Clash, Nyanpasu (Enhance) |
|          | Ports & Network    | Ports (mixed / socks / http), LAN and IPv6 (allow LAN, IPv6), TUN stack                                                                           | Clash                     |
|          | Control Channel    | Channel (prefer IPC / HTTP only, disable the HTTP controller), HTTP external controller (address, port strategy, secret), Web UI                  | Clash, External Control   |
|          | Runtime Config     | Field filter (switch, allowed fields), expand include-all, built-in script enhancements                                                           | Clash, Nyanpasu (Enhance) |
| Nyanpasu | User Interface     | Language, theme mode and color, custom CSS                                                                                                        | User Interface (as is)    |
|          | Tray & Windows     | Tray menu (mode, proxy group display), tray icon (not on macOS), window close behavior                                                            | Nyanpasu                  |
|          | Widget             | Network statistic widget (kind, logo follows state, icon source, icon config)                                                                     | Nyanpasu                  |
|          | Keyboard Shortcuts | `HotkeyManager`                                                                                                                                   | Nyanpasu                  |
|          | Logs & History     | App logs (level, file count and size), core logs (level, storage compression and size), traffic history (retention, local IP detection)           | Nyanpasu, Clash           |
|          | Updates            | Update status and actions (`UpdateControls`), update sources, update preferences (auto check / download, release channel)                         | About                     |
|          | Debug Tools        | Unchanged                                                                                                                                         | —                         |
| About    | Clash Nyanpasu     | Version hero (logo, name, version) and help links (wiki, issues, GitHub)                                                                          | About, header help menu   |
|          | Licenses           | The current credits page, unchanged                                                                                                               | About (child page)        |

### 4.3 Rationale for the main decisions

- **Separate Core Manager from Core Settings.** Core management is a set of
  actions (switch, update, restart), not settings, and it is where the
  dashboard's current-core card leads. People open it for a different reason
  than to change values.
- **Merge the external controller and Web UI into Control Channel.**
  `clash_ipc_disable_http_controller` turns off the HTTP external controller,
  and Web UI connects through that controller. On separate pages, the effect of
  changing one is visible only on the other page. The "Clash External Control"
  page goes away.
- **Put LAN, IPv6, and TUN stack with the ports.** All of them decide where the
  core receives traffic; allowing LAN changes the address the ports listen on.
- **Dissolve Enhance.** "Close connections when switching" acts on the core's
  connections, so it moves to Core Settings. "Built-in script enhancements" is
  a rule for building the runtime config from a profile, like the field filter
  and include-all, so it moves to Runtime Config.
- **Keep logs in one place.** App log and core log storage both decide what is
  kept and how much, and they are easier to find together. The core log level
  is a Clash config field, but to users it is a log setting, so it goes here as
  well. Traffic history retention is the same kind of setting and joins them.
- **Split System in two.** Turning the proxy on and off and changing its mode
  happens daily; installing the service and setting auto-start happens once.
  Service control (577 lines) is also independent of the proxy settings.
- **Updates and Debug Tools in the Nyanpasu group.** Checking for and
  installing a new version, the update source, and the release channel are
  things the user configures and runs for the app. Debug Tools inspects the
  app itself and is used rarely, so it closes the group.
- **An About group of two.** The version page and the license list form the
  usual "about this app" pair. The first item is labeled with the app name and
  shows the version as its description, like Android's "About phone — Pixel 9".
- **Keep the About, Licenses, and Debug paths.** `/main/settings/about`,
  `/main/settings/about/credits`, and `/main/settings/debug` keep working, and
  the backend's debug HTTP server tests use `/main/settings/debug`. The "View
  licenses" card on About goes away, because the sidebar item replaces it.

## 5. Sidebar design (M3E)

### 5.1 Structure

```text
┌────────────────────────────────┐
│  System                        │  ← group heading (label-medium, primary)
│ ╭────────────────────────────╮ │
│ │ (◉) Proxy                   │ │  ← first item: large radius on top corners only
│ │     Routing mode, …         │ │
│ ├────────────────────────────┤ │  ← 2px gap
│ │ (◉) Service & Startup       │ │  ← last item: large radius on bottom corners only
│ │     Service mode, start …   │ │
│ ╰────────────────────────────╯ │
│                                │  ← 16px between groups
│  Clash                         │
│ ╭────────────────────────────╮ │
│ │ (◉) Core Manager            │ │
│ ├────────────────────────────┤ │
│ │▐(✿) Core Settings          ▌│ │  ← selected: secondary-container, corners morph round
│ ├────────────────────────────┤ │
│ │ …                          │ │
└────────────────────────────────┘
```

As in Android 17 Settings, a group is a stack of items separated by small gaps,
with large radii only at the start and end of the stack. `SettingsGroup` inside
setting pages already follows the same radius rule (large at the ends, small in
between), so the sidebar and the pages share one shape language.

### 5.2 Specification

| Element             | Specification                                                                                                                                                                                                                                                                                                                    |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sidebar background  | Light: `bg-surface-variant/50`; dark: no fill (the app background). In the light scheme `surface` equals `background`, so items need a tinted ground to stand out, as in Android.                                                                                                                                                |
| Group heading       | `text-primary`, label-medium equivalent (`text-sm font-medium`), 16px horizontal and 8px vertical padding. Rendered as `<h2>`; the group gets `role="group"` and `aria-labelledby`.                                                                                                                                              |
| Item container      | 2px between items (`gap-0.5`). First item: 24px top / 4px bottom radius; middle items: 4px; last item: 4px top / 24px bottom; a single item: 24px.                                                                                                                                                                               |
| Item                | At least 64px tall. Background `bg-surface` in both modes. 16px horizontal padding and 12px between icon and text.                                                                                                                                                                                                               |
| Icon                | A 32px circle in the item's own color (§5.3): `bg-<color>-container` with a 20px glyph in `text-on-<color>-container`, the size of the previous sidebar's icons.                                                                                                                                                                 |
| Title / description | Title `text-sm font-medium`, description `text-xs text-on-surface-variant` on one line, as in the previous sidebar. Overflow scrolls through the existing `TextMarquee`.                                                                                                                                                         |
| Hover / press       | `on-surface/8` state layer and the existing `Button` ripple.                                                                                                                                                                                                                                                                     |
| Focus               | `focus-visible:ring-2 ring-primary ring-inset` (same as `navigation-sidebar`).                                                                                                                                                                                                                                                   |
| Selected            | Background `secondary-container`, text `on-secondary-container`. Radius becomes 24px on every corner regardless of position (M3E shape morph). The icon morphs from a circle to a cookie through `MaterialShape` and switches to a `primary` fill with an `on-primary` glyph. Switches instantly under `prefers-reduced-motion`. |
| Width               | Slightly wider than before: 1:2.5 with the content (was 1:3), at least 288px (`min-w-72`) and at most `max-w-96`.                                                                                                                                                                                                                |

### 5.3 Item colors

Each item has its own icon color, as in Android Settings. The theme's tonal
roles give only five container colors, which is not enough for 16 items, so
`@nyanpasu/theme` adds eight fixed hues as static tokens: `red`, `orange`,
`yellow`, `green`, `cyan`, `blue`, `purple`, `pink`. Each has
`--color-md-<hue>-container` and `--color-md-on-<hue>-container` for light
(tones 90 / 10) and dark (tones 30 / 90), computed with
`material-color-utilities`' `customColor` without harmonization, so the hues
stay apart whatever the theme seed is. Tailwind maps them to
`bg-<hue>-container` / `text-on-<hue>-container`. A ninth, neutral color reuses
`surface-variant` / `on-surface-variant`.

| Group    | Item               | Color   |
| -------- | ------------------ | ------- |
| System   | Proxy              | blue    |
|          | Service & Startup  | green   |
| Clash    | Core Manager       | purple  |
|          | Core Settings      | orange  |
|          | Ports & Network    | cyan    |
|          | Control Channel    | blue    |
|          | Runtime Config     | yellow  |
| Nyanpasu | User Interface     | pink    |
|          | Tray & Windows     | cyan    |
|          | Widget             | green   |
|          | Keyboard Shortcuts | neutral |
|          | Logs & History     | orange  |
|          | Updates            | purple  |
|          | Debug Tools        | red     |
| About    | Clash Nyanpasu     | neutral |
|          | Licenses           | yellow  |

### 5.4 Behavior

- Selection uses a per-item `fuzzy` match, so an item stays selected on its
  child pages. The Clash Nyanpasu item matches exactly: Licenses lives under
  its path (`/about/credits`).
- The active link gets `aria-current="page"`; the sidebar is a `<nav>` with an
  `aria-label`.
- On mobile (`useIsMobile`), the `/main/settings` index still shows the same
  navigation full-width, and each page's `PageTitle backTo="/main/settings"`
  returns to it.
- No item becomes empty on any platform (the tray icon is hidden on macOS, but
  the tray menu and window settings remain), so items are not filtered by
  platform.
- The search phase adds a search field above the first group. Phases 1 and 2
  leave no empty placeholder for it.

### 5.5 Components

Only the settings pages use this navigation, so it stays in
`settings/_modules/settings-navigate.tsx` rather than `@nyanpasu/ui` (no
abstraction for a single consumer). Groups and items are composed in JSX:

```tsx
<SettingsNavigation aria-label={m.settings_navigation()}>
  <SettingsNavigationGroup label={m.settings_group_clash()}>
    <SettingsNavigationItem
      to="/main/settings/clash/core-manager"
      color="purple"
      icon={<DeployedCodeRounded />}
      label={m.settings_label_core_manager()}
      description={m.settings_label_core_manager_description()}
    />
    …
  </SettingsNavigationGroup>
</SettingsNavigation>
```

Styles are `cva` definitions such as `settingsNavigationItemVariants`. The item
position (first, middle, last) comes from CSS sibling selectors on the group,
like `SettingsGroup`, not from props. Slots: `settings-navigation`,
`settings-navigation-group`, `settings-navigation-group-label`,
`settings-navigation-item`; the icon keeps `MaterialShape`'s own `material-shape`
slot. Existing slots such
as `settings-sidebar-scroll-area` stay.

Icons (`material-symbols`): Proxy `vpn-lock-rounded`, Service & Startup
`power-settings-new-rounded`, Core Manager `deployed-code-rounded`, Core
Settings `tune-rounded`, Ports & Network `lan-rounded`, Control Channel
`swap-horiz-rounded`, Runtime Config `data-object-rounded`, User Interface
`palette-rounded`, Tray & Windows `desktop-windows-rounded`, Widget
`widgets-rounded`, Keyboard Shortcuts `keyboard-rounded`, Logs & History
`receipt-long-rounded`, Updates `system-update-alt-rounded`, Debug Tools
`bug-report-rounded`, Clash Nyanpasu the Nyanpasu logo, Licenses
`license-rounded`.

The Nyanpasu logo needs two adjustments that the Material Symbols glyphs do
not:

- **Fill the face.** The logo is a solid silhouette with the face (`#bg`)
  drawn over it and the eyes and mouth (`#element`) drawn on the face. Fill
  `#bg` with the icon's own background (`surface-variant` at rest, `primary`
  when selected); a transparent face lets the silhouette show through, and
  the eyes and mouth disappear into it.
- **Center it optically, not geometrically.** The cat's visual mass sits left
  of and below the center of its box: the tail on the right and the ears on
  top are thin, and the centroid of its silhouette is at (44%, 58%) of the
  box. Centered by its box, the logo looks shifted left and down inside the
  circle, so it gets an offset toward the opposite side: `translate-x-[6%]`
  and `-translate-y-[6%]` of its own size. The vertical offset is smaller than
  the centroid suggests because the outlined face reads lighter than a filled
  shape. Any other place that puts the logo in a small round container needs
  the same kind of offset, checked by eye at the rendered size.

The [mockup](https://claude.ai/artifact/Kn7UEVAA2gKCcvixufFYk3) (a private
Design canvas) shows the sidebar in the app window in light and dark, the
full sidebar, the mobile list, and the item states, in the app's default
scheme (seed `#3e64a7`). Clicking an item shows that page's sections with
placeholder values.

## 6. Routing

### 6.1 Route layout

Groups become URL segments. A group has no layout route of its own: each item
page is a direct child of the settings route, so the settings outlet animates
every move between items.

```text
settings/
  route.tsx              sidebar + content (existing)
  index.tsx              mobile navigation (existing)
  _modules/              shared across settings (settings-card, settings-navigate, …)
  system/
    proxy/index.tsx      + proxy/_modules/
    service/index.tsx    + service/_modules/
  clash/
    core-manager/  core/  network/  control/  runtime/
  nyanpasu/
    interface/  tray/  widget/  shortcuts/  logs/  updates/
  about/
    route.tsx            unchanged
    index.tsx            Clash Nyanpasu (version hero and help links)
    credits/index.tsx    Licenses (unchanged)
  debug/                 unchanged
```

Each item page follows the routed-page rules: its `index.tsx` owns the page
composition, and modules used only by that page live in its `_modules/`.

### 6.2 Old URLs and callers

The old paths `/main/settings/system`, `/clash`, `/user-interface`, `/web-ui`,
and `/nyanpasu` are removed without redirects. In-app callers move to the new
paths:

- `pages/(main)/_modules/navbar.tsx` (desktop and mobile settings tabs) →
  `/system/proxy`
- `components/widgets/widget-shortcut.tsx`: proxy status chip →
  `/system/proxy`, current-core card → `/clash/core-manager`
- `pages/(main)/_modules/header-menu.tsx`: the "new version" / "update ready"
  button → `/nyanpasu/updates` (the help menu's About link stays on `/about`)
- Tests: `settings-navigate.browser.test.tsx`,
  `perf/settings-switch.bench.test.tsx`, `about-update-layout.browser.test.tsx`
  (renamed `updates-layout`, its checks move to the Updates page), a new
  `about-page` test for the version and help links, and the browser tests
  that import `_modules` directly

## 7. Module placement and i18n

- Page-only modules move with `git mv` into that page's `_modules/` (for
  example `clash/_modules/core-manager-card.tsx` →
  `clash/core-manager/_modules/`). Only modules used by two or more pages go
  into the nearest common ancestor's `_modules/`. Contents stay the same; only
  import paths change.
- `nyanpasu/_modules/log-level-selector.tsx` and
  `clash/_modules/log-level-selector.tsx` are different components with the
  same name. Both land on Logs & History, so they are renamed to
  `app-log-level-selector.tsx` and `core-log-level-selector.tsx`.
- `UpdateControls` leaves `NyanpasuVersion` and, with `update-source-selector`,
  `update-preferences-card`, `release-channel-preference`, and
  `about-card-header`, moves to `nyanpasu/updates/_modules/`. A new
  `update-status-card` gives the controls a header with the current version.
  `NyanpasuVersion` keeps only the hero. `open-source-credits-card.tsx` and its
  `settings_about_credits_view` message are removed.
- The help links on About reuse the header help menu's targets: the wiki
  (`https://nyanpasu.org`), a new issue with the environment details, and the
  GitHub repository, shown as setting rows under a Help heading. The link
  logic moves to `pages/(main)/_modules/help-links.ts`, the smallest common
  scope of the help menu and the About page.
- New message keys: group headings
  `settings_group_{system,clash,nyanpasu,about}`, item labels and descriptions
  `settings_label_<item>` / `settings_label_<item>_description`, and
  `settings_navigation` (the nav `aria-label`). Section headings inside pages
  reuse existing keys (such as `settings_clash_settings_port_label`). Keys
  that become unused (`settings_label_external_controll*` and others) are
  removed. New keys go next to their related keys in all six locales.

## 8. Implementation phases and commits

| Phase | Content                                                                                                                                                                                                                          | Verification                                                                                                                                           |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1a    | Add the eight item color tokens to `@nyanpasu/theme` and Tailwind.                                                                                                                                                               | `pnpm typecheck`, Stylelint, visual check.                                                                                                             |
| 1b    | Rebuild the sidebar in the M3E design and arrange the **current pages** in groups (System: System; Clash: Clash / External Control; Nyanpasu: User Interface / Nyanpasu / Debug; About: About / Licenses). Routes do not change. | Extend the `settings-navigate` test with group headings, `aria-current`, and selection on child pages. Visual check in light / dark, desktop / mobile. |
| 2a    | Split System (proxy / service), update navbar and widget links.                                                                                                                                                                  | `pnpm typecheck`, oxlint, tests, navigation in the app.                                                                                                |
| 2b    | Split Clash into five pages, merging External Control and part of Enhance.                                                                                                                                                       | As above, plus the tests of moved modules and the settings-switch perf bench.                                                                          |
| 2c    | Create the Nyanpasu group pages (move UI, split Nyanpasu, merge logs).                                                                                                                                                           | As above.                                                                                                                                              |
| 2d    | Split Updates out of About, point the header update button at it, add the help links to About, remove the licenses card.                                                                                                         | As above, plus the moved update tests.                                                                                                                 |
| 3     | Settings search. Designed separately once Phase 2 has settled the pages.                                                                                                                                                         | —                                                                                                                                                      |

Each commit builds and passes tests on its own. 2a–2d apply in order, and each
updates its group's navigation items in the same commit, so the sidebar and
links stay consistent in between. Pages that use `useDeferredValue` because
they are long drop it once the split makes it unnecessary.
