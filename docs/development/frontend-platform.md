# Frontend platform facts and adapters

Use `@nyanpasu/platform` as the single frontend boundary for runtime facts and local browser or native capabilities. Keep platform facts separate from backend capabilities and viewport layout.

## Facts

`PlatformInfo` is the frontend snapshot:

- `surface` is `desktop`, `browser`, or `unknown`. A browser connecting to a desktop backend is still a browser surface. Current Tauri webviews are desktop surfaces; a future mobile shell must identify its surface explicitly.
- `clientOs` is `windows`, `macos`, `linux`, or `unknown`. It identifies the device rendering the frontend and is best effort for browsers.
- `RuntimeInfo.bridge` is `tauri` or `none`. It selects local IPC and native adapters independently from surface.

`detectRuntime()` is the runtime probe. It does not use Vite's build machine OS as a client OS fallback. Mobile user-agent hints remain `unknown` for the three desktop operating systems. `useIsMobile` and breakpoint hooks describe viewport width; do not use them to infer OS or shell.

When a feature needs to know the backend host, use a backend query or add an explicit RPC fact. The local snapshot never assumes the backend runs on the same OS as a WebUI client. Packaging facts such as AppImage and portable mode remain backend queries.

## Matching and React

Use the pure `matchesPlatform(info, condition)` for non-React code and `matchPlatform(info, implementations)` to select a callback. String OS keys (`windows`, `macos`, `linux`) mean a desktop app on that OS; use an object such as `{ surface: 'browser', clientOs: 'windows' }` when the client OS matters in a browser. Condition arrays are OR; object fields are AND. `matchPlatform` checks the matching desktop OS before the `desktop` fallback, then invokes the first matching case. It preserves the selected callback's result and does not retry a fallback after a callback throws.

Use the `@nyanpasu/platform/react` subpath for `PlatformProvider`, `usePlatform`, `useMatchesPlatform`, and the `Platform*` render gates. The app creates one runtime snapshot in `main.tsx` and provides it above the router. Tests and embedded component harnesses can wrap the subject in `<PlatformProvider platform={...}>` to make their environment explicit. The hook uses the shared runtime probe when rendered outside a provider; production app routes use the provided snapshot.

A render gate does not protect module-scope expressions or parent render work. Keep native resource access inside `createWindowAdapter()` or another lazy adapter, and call it only where that capability is used. A missing adapter is an unavailable capability, not an OS guess.

## Transport and local adapters

`@nyanpasu/rpc` selects command and event transports once when the client is created, from the same `RuntimeInfo`: Tauri IPC for a Tauri bridge, HTTP/SSE for an identified browser, and no real transport for an unknown surface. Its connection-details adapter selects Tauri Channel or dedicated SSE from that same runtime. Query owns subscription demand and UI state; it does not detect Tauri or construct EventSource/Channel objects. Inject a transport in tests. A failed request does not retry over another transport.

Use `createWindowAdapter()` for Tauri window operations and `createBrowserAdapter()` for browser dialogs, navigation, external links, and downloads. These factories return `null` when their surface is unavailable and do not obtain native window handles at import time. Keep browser file and clipboard APIs behind the same runtime decision in the app's interaction utilities. Backend authorization and operation support still belong to RPC; platform-based hiding is not an authorization check.

## Review checklist

- Search frontend source for raw Tauri detection and imports of `getCurrentWebviewWindow` outside `@nyanpasu/platform`.
- Confirm every gate uses the required dimension: surface, client OS, bridge, backend capability, or viewport.
- Check that browser actions remain available where intended and native commands, writes, and subscriptions are not issued from the browser.
- Exercise desktop and browser snapshots with matching and mismatching OS hints, plus an unknown snapshot. Test that excluded children do not mount.
- Keep `OS_PLATFORM` and client OS globals out of build config. Use backend queries for package behavior and host-specific capability data.
