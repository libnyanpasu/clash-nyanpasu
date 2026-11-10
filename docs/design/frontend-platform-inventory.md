# Frontend platform migration inventory

This inventory accompanies the [frontend platform design](frontend-platform.md).
It was checked against `main` at `ba4c4dc6f`. The baseline contains **32 files**
that directly reference the legacy platform flags or Vite defines: 28 in the
app (including configuration and declarations), one in Platform, two in RPC,
and one in Query. Tests, generated bindings, and performance configurations
are excluded from the count; their relevant migration work is listed below.

App paths are relative to `frontend/nyanpasu/src/`. Paths beginning with
`frontend/` are relative to the repository root.

## Runtime detection and transport (7 files)

| File                                                                     | Current decision                                                                                                                                                   | Migration and behavior to preserve                                                                                                                                                                                      |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/platform/src/index.ts`                                         | Exports Tauri and browser checks and derives static OS flags from the user agent and `OS_PLATFORM`. Also provides native window and browser notification adapters. | Keep raw runtime detection in this package. Produce an explicit snapshot and bridge identity. Keep native resource access lazy and remove the legacy flag exports after migrating consumers.                            |
| `frontend/nyanpasu/vite.config.ts`, `services/types.d.ts`                | Inject or declare `OS_PLATFORM` and `WIN_PORTABLE`.                                                                                                                | Drop the build machine OS fallback. If desktop packaging supplies an OS hint, use the actual target. Drop `WIN_PORTABLE` once notifications use the running instance's behavior. Update the performance config as well. |
| `frontend/rpc/src/transport.ts`, `frontend/rpc/src/command-transport.ts` | Events choose Tauri or HTTP once, while commands check Tauri at each invocation.                                                                                   | Select IPC or HTTP/SSE once for the entire client. Preserve explicit transport injection and wire semantics. Unknown runtimes must not attempt a real transport.                                                        |
| `frontend/query/src/provider/clash-connection-details-provider.tsx`      | Uses a Tauri Channel or the dedicated `/bridge/connection-details` SSE route.                                                                                      | Inject the stream adapter selected by the RPC boundary. Keep demand, connection state, retry, and freeze in Query. Close SSE after the last consumer and unsubscribe if a native subscription resolves after cleanup.   |
| `pages/__root.tsx`                                                       | Redirects the browser root, gets a native window at import time, and uses Tauri for window-ready, error-page close, and main-window deep links.                    | Read the router snapshot for the redirect and use a lazy window adapter for native actions. Keep the desktop error window visible by reporting ready, and preserve the `main` window label check.                       |

## Window behavior and layout (9 files)

| File                                                                                                                              | Current decision                                                                 | Migration and behavior to preserve                                                                                                                                                               |
| --------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `pages/(main)/_modules/header.tsx`, `pages/(editor)/editor/_modules/header.tsx`                                                   | Choose a macOS title bar using only the OS flag.                                 | Use a macOS desktop gate; browsers use the generic layout. Check menu access, window buttons, and drag regions.                                                                                  |
| `pages/(editor)/editor/_modules/utils.tsx`                                                                                        | Adds Twemoji to the Windows font stack.                                          | Derive the font stack from `clientOs`, including Windows browsers when this option is used there. Pass the snapshot into non-React editor setup instead of baking a flag into a module constant. |
| `components/window/window-control.tsx`, `hooks/use-window-maximized.ts`                                                           | Acquires native window controls; on macOS reads fullscreen state as maximized.   | Use a lazy window adapter and preserve the macOS check and resize throttling. Migrate the theme provider's existing window adapter consumer too; browser color scheme stays a media query.       |
| `pages/(editor)/editor/profile/index.tsx`, `pages/(editor)/editor/css/index.tsx`, `pages/(tray-menu)/tray-menu/_modules/hooks.ts` | Obtain the current webview at module import to close it after an action.         | Obtain window capability on demand. Preserve save and close order and errors. Check direct WebUI navigation without assuming all editor operations are native-only.                              |
| `pages/(main)/main/settings/debug/_modules/window-debug.tsx`                                                                      | Gets the window label at import time and shows a browser label without a window. | Read the label through the adapter and provide an explicit non-native diagnostic state.                                                                                                          |

## Feature availability (9 files)

| File                                                                                                                                                                                                                       | Current decision                                                                                    | Migration and behavior to preserve                                                                                                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pages/(main)/main/settings/system/service/index.tsx`, `pages/(main)/main/settings/system/service/_modules/system-service-ctrl.tsx`                                                                                        | Shows UWP and system tools based on OS; generates a service command with or without `sudo`.         | Gate the tools on the relevant desktop surfaces. Construct commands for the host running the service, not the WebUI device. Do not emit a guessed command when the host is unknown. Check direct WebUI navigation for native-only requests. |
| `pages/(main)/main/settings/nyanpasu/tray/index.tsx`, `pages/(main)/main/settings/nyanpasu/_modules/icon-set-store.tsx`                                                                                                    | Shows the tray icon editor outside macOS; offers presets on Windows.                                | Use positive desktop conditions for the tray editor. Derive preset availability when composing the tray store, and preserve the widget store's independent behavior.                                                                        |
| `components/providers/nyanpasu-update-provider.tsx`, `pages/(main)/main/settings/nyanpasu/updates/_modules/update-source-selector.tsx`, `pages/(main)/main/settings/nyanpasu/updates/_modules/update-preferences-card.tsx` | Enables desktop update queries and shows an explanatory message instead of controls in the browser. | Use the shared desktop condition and preserve the backend's `supported` value and update states in the consuming controls. Unknown and browser surfaces must not mount the native update controls.                                          |
| `pages/(main)/main/settings/nyanpasu/public-ip/_modules/stun-server-list.tsx`                                                                                                                                              | Hides the probe in browsers; `test_stun_server` is not exposed over HTTP.                           | Gate the probe and handler positively on the desktop surface. Preserve the browser-accessible settings and explanation.                                                                                                                     |
| `pages/(main)/main/settings/debug/_modules/http-server.tsx`                                                                                                                                                                | Reads HTTP server status on both surfaces; toggling and native link opening are desktop-only.       | Keep browser reads and browser link behavior. Gate only writes and native actions. The backend exposes `get_debug_http_status` over HTTP, but does not expose `set_debug_http_enabled` or `open_web_url`.                                   |

## Local interaction and path data (7 files)

| File                                                                                          | Current decision                                                                                                                | Migration and behavior to preserve                                                                                                                                                               |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `utils/file-picker.ts`, `utils/clipboard.ts`                                                  | Native file selection returns a path; browser selection returns a `File`. Clipboard uses native RPC or the Web Clipboard API.   | Select the implementations at app composition. Preserve cancellation, clipboard permission errors, and the distinction between paths and files. Do not access the DOM in an unknown environment. |
| `utils/notification.ts`                                                                       | Uses web notifications and dialogs, native notifications and dialogs, and a portable Windows notification fallback.             | Compose the appropriate adapters. Keep the portable fallback based on running package behavior, and preserve error details, clipboard copy, and profile label lookup.                            |
| `pages/(main)/_modules/help-links.ts`, `pages/(main)/main/settings/_modules/open-external.ts` | Uses desktop RPC for external links. The browser paths intentionally navigate the current page or open a new one, respectively. | Share an external-link adapter with an explicit navigation intent. Keep `noopener,noreferrer` for new tabs and preserve issue-report diagnostics.                                                |
| `pages/(main)/_modules/header-help-action.tsx`                                                | Calls desktop log collection or starts the browser download at `/bridge/logs/archive`.                                          | Move this choice behind the log export adapter. Preserve the existing HTTP download route, session behavior, and archive naming.                                                                 |
| `components/ui/file-drop-zone.tsx`                                                            | Picks a separator using the client OS and prefers `File.name` when available.                                                   | Use the file name or the format of the source path. Cover Windows and POSIX paths without treating a valid POSIX backslash as a separator.                                                       |

## Related checks and verification

- `useIsMobile` and shared sidebar layout use viewport breakpoints. Theme
  preference, DOM existence checks, and WebGL/WebGPU and user-agent diagnostics
  are also separate concerns. Native drag-region attributes are not runtime
  probes. Node release and build scripts still use their own host platform.
- `frontend/query/src/ipc/use-platform.ts` queries the backend for AppImage
  and portable packaging. Those queries describe the running backend instance;
  they do not belong in the local platform snapshot.
- Replace business-level flag mocks with snapshot or adapter injection. Update
  the app's browser tests, RPC command transport tests, connection details
  browser tests, and the performance configuration. Keep Tauri globals only
  in tests that exercise the real bridge, and clean them up between tests.
- Repeat the search when closing the migration, and also look for raw Tauri
  detection imports, current-window access, and user-agent checks. The expected
  remaining hits should be confined to intentional runtime detection, native
  adapters, diagnostics, generated code, or tests of those boundaries.

Reproduce the baseline from the repository root:

```sh
rg -l --glob '*.{ts,tsx}' \
  --glob '!**/{tests,perf,node_modules,dist}/**' \
  --glob '!**/*bindings.ts' \
  '\b(isTauri|isBrowser|isWindows|isMacOS|isLinux|OS_PLATFORM|WIN_PORTABLE)\b' frontend
```
