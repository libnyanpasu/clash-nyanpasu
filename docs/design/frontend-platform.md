# Frontend platform detection and branching

Status: draft for discussion.

The frontend already has a platform package, but consumers use its OS flags for
different jobs, and RPC and connection details each detect Tauri independently.
For example, the system service page shows Windows tools based on the client OS
alone, even if the frontend is running as a WebUI. Move all existing consumers to
one source of platform facts, then remove the legacy flags. The
[migration inventory](frontend-platform-inventory.md) records the call sites on
main and the behavior each migration must preserve.

## Model

| Fact             | Meaning and ownership                                                                                                                                                                                                          |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Frontend surface | `desktop`, `browser`, or `unknown`. Used for application layout and local interaction. A browser connected to a desktop process is still a browser.                                                                            |
| Client OS        | `windows`, `macos`, `linux`, or `unknown`. This describes the device running the frontend. A browser's OS hint can inform presentation and keyboard or font choices.                                                           |
| Native bridge    | Whether a Tauri bridge is present. Used by the composition layer to select local adapters and RPC transport. It is independent of the frontend surface so a future mobile shell can use the bridge without matching `desktop`. |
| Backend host     | The environment running the service. When a feature needs to distinguish hosts, get the information from the backend through unified RPC. The first version does not need this field.                                          |
| Capability       | Whether a local adapter or backend operation is available. Use the adapter or the relevant backend query. Keep loading, error, and unsupported states distinct; backend authorization remains with RPC.                        |

Use Tauri's existing detector at the runtime boundary. For the current native
app, a Tauri webview is a desktop surface; a future mobile entry point must
declare its surface explicitly. A window outside Tauri is a browser surface. If
there is no runtime window, use `unknown`. Viewport breakpoints such as
`useIsMobile` are independent of these facts.

Use known runtime information or an explicit packaging target for desktop OS
detection, with recognizable WebView hints as a fallback. Browser OS hints are
best effort; keep unrecognized or explicitly mobile environments out of desktop
OS classifications. Remove the Vite `OS_PLATFORM` fallback: it currently
describes the machine running the build, and the same frontend assets can run
on another OS.

## React components and hooks

```tsx
<>
  <PlatformDesktop>
    <PlatformWindows>
      <WindowsTools />
    </PlatformWindows>
    <PlatformMacos>
      <MacosTools />
    </PlatformMacos>
    <PlatformLinux>
      <LinuxTools />
    </PlatformLinux>
  </PlatformDesktop>

  <Platform when={['windows', 'macos']}>
    <SharedSystemTools />
  </Platform>

  <PlatformBrowser>
    <WebUiActions />
  </PlatformBrowser>
</>
```

- Named components work on their own. `PlatformWindows`, `PlatformMacos`, and
  `PlatformLinux` mean **desktop app on that OS**. The corresponding string
  shortcuts have the same semantics. Use `{ clientOs: 'windows' }` when only
  the client OS matters, or `{ surface: 'browser', clientOs: 'windows' }` to
  narrow that condition to the WebUI.
- A condition list is OR; fields in a condition and nested components are AND.
  Sibling components render independently in JSX order. The generic component
  requires `when`; use a fragment for grouping. There is no extra DOM element.
  An optional `fallback` renders when that gate fails; the default is `null`.
- A gate prevents children from mounting. It cannot prevent evaluation of
  module-scope code or expressions evaluated by the parent. Put a native hook
  inside the gated child, and defer native resource access in the adapter.

```ts
type Surface = 'desktop' | 'browser' | 'unknown'
type ClientOs = 'windows' | 'macos' | 'linux' | 'unknown'

type PlatformInfo = Readonly<{
  surface: Surface
  clientOs: ClientOs
}>

type PlatformKey = 'desktop' | 'browser' | Exclude<ClientOs, 'unknown'>
type PlatformSelector =
  | PlatformKey
  | { surface: Surface; clientOs?: ClientOs }
  | { surface?: Surface; clientOs: ClientOs }
type PlatformCondition =
  PlatformSelector | readonly [PlatformSelector, ...PlatformSelector[]]
```

The app supplies the same stable snapshot to the router context and to
`<PlatformProvider platform={platform}>` above the router. The React API has
two general hooks:

| Hook                                                   | Use                                                                                                           |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| `usePlatform(): PlatformInfo`                          | Read the snapshot when a component or hook needs to pass it to a helper or derive a platform-dependent value. |
| `useMatchesPlatform(when: PlatformCondition): boolean` | Use the shared matcher for a boolean such as query `enabled`, menu visibility, or an interaction guard.       |

The hooks read the nearest provider and do not probe the environment or fetch
backend data. A missing provider is a wiring error; an explicitly unknown
snapshot is valid. Tests can inject either desktop or browser facts on the same
host. Route loaders and ordinary TypeScript functions receive the snapshot
explicitly. Keep hooks unconditional: for example, the debug HTTP card can
read `useMatchesPlatform('desktop')` to enable writes while continuing to run
its read query in the WebUI. A query's `enabled` flag controls automatic
execution; explicit refetches, mutations, and native calls still need their
own appropriate guards.

There is no need for separate `useIsWindows`, `useIsTauri`, or
`useMatchPlatform` hooks. A feature hook may combine platform facts, an
adapter, and existing query results. For example, `useWindowMaximized` can
read the platform once and, after checking that its window adapter exists,
select the macOS fullscreen check or the regular maximized check.

## TypeScript matching

`matchesPlatform(platform, when)` returns a boolean and is the implementation
used by the React gates. Use `matchPlatform` to select and call one
implementation:

```ts
const file = await matchPlatform(platform, {
  desktop: pickNativeFile,
  browser: pickBrowserFile,
  default: () => null,
})
```

A matching desktop OS key takes precedence over the `desktop` key, regardless
of object order. A desktop with an unknown OS can still match `desktop`. A
browser does not match the desktop OS keys. For grouped or compound conditions,
use an ordered list:

```ts
const label = matchPlatform(platform, {
  cases: [
    { when: ['windows', 'macos'], run: getSharedDesktopLabel },
    { when: 'desktop', run: getGenericDesktopLabel },
    {
      when: { surface: 'browser', clientOs: 'macos' },
      run: getMacBrowserLabel,
    },
    { when: 'browser', run: getBrowserLabel },
  ],
  default: getGenericLabel,
})
```

The `cases` form uses the first match and cannot be combined with the shorthand
keys. Invoke only the selected callback. Preserve its return value (including a
Promise) and infer the union of branch return types. If nothing matches, invoke
`default` if present; otherwise return `undefined`. A selected callback
returning `undefined`, throwing, or rejecting does not trigger the fallback.
Use callbacks to choose a value or implementation; put native resource ownership
and repeated adapter selection at the composition boundary.

## Composition, RPC, and WebUI

- Keep the `@nyanpasu/platform` root entry React-free. It owns the matcher,
  types, runtime probe, and local adapters. Export the provider, hooks, and
  components through `@nyanpasu/platform/react`. The platform package does
  not depend on RPC, Query, or the app. Shared UI primitives continue to take
  derived props.
- Let the app composition layer produce a runtime description containing
  `platform: PlatformInfo` and `bridge: 'tauri' | 'none'`. The app passes the
  same facts to the router, React, and service factories. Keep this composition
  below the route tree to avoid an import cycle with `main.tsx`. Obtain window
  handles and other native resources lazily.
- Select command and event transports once when creating the RPC client.
  Currently commands recheck Tauri on every call and events check at
  construction. Both should use the same selection: Tauri IPC when the bridge
  is present, HTTP/SSE for an explicitly identified browser. Unknown
  environments do not select a real transport. Standalone default factories
  can call the same probe, while tests and independent clients can supply
  adapters explicitly. A failed RPC call must not retry over another transport.
- Use the same transport selection for the dedicated connection details stream.
  Inject the subscription adapter into the Query provider. The RPC boundary
  owns Tauri Channel vs. dedicated SSE and cancellation; the provider keeps
  demand, state, retry, and freeze behavior. Preserve teardown after the last
  consumer and unsubscribe from native subscriptions that resolve after
  cleanup. Keep the ordinary event stream and existing HTTP allowlist intact.

The existing [OpenWrt MVP](openwrt-mvp.md) uses native LuCI through rpcd, CLI,
and a local socket. Its [roadmap](openwrt-roadmap.md) is paused. This design
does not add `PlatformOpenWrt` or require a React deployment on the router.
If a WebUI later connects to an OpenWrt service, the frontend remains a
browser surface. Use backend capability data for actions; introduce explicit
host metadata only when a feature needs to know the host identity. This follows
the [application shell and RPC boundaries](application-platform-extraction.md#rpc-targets).

## Migration and acceptance

1. Add the runtime probe, pure matching, React entry, and app composition.
   Pilot the API with system service tools, the window maximize hook, and
   file selection. Verify same-OS browser and desktop snapshots produce
   different native gates, and that the component and hook agree with the
   pure matcher. Cover OR, nesting, fallback, branch precedence, and
   non-mounting of excluded children.
2. Migrate the RPC command/event split, the dedicated connection details
   subscription, and the native window and interaction adapters. Preserve
   lifecycle and transport behavior. Verify browser imports do not acquire
   a window handle, window-ready is reported from the desktop error boundary,
   and deep-link import is limited to the main window.
3. Close the remaining rows in the [inventory](frontend-platform-inventory.md).
   Test direct WebUI navigation as well as hidden controls. Preserve shared
   read operations, and verify desktop-only queries, subscriptions, and
   writes are not sent by the browser. Test desktop and browser on Windows,
   macOS, and Linux, and cover an unknown surface and desktop with unknown
   OS using injected snapshots.
4. Remove the old business exports and `OS_PLATFORM`, and remove
   `WIN_PORTABLE` after its notification behavior has an instance-based
   replacement. Recheck raw probes, native window access, and stale test
   stubs; constrain direct native imports in business code using existing
   import lint rules where applicable. Run focused `pnpm run test:frontend`
   tests, `pnpm run typecheck`, and `pnpm run lint:frontend-boundaries`.
   Verify the three desktop shells and a running WebUI before closing the
   migration.

## Development guide changes to land with the API

Update [TypeScript and React](../development/typescript.md#reuse-constants-and-environment-checks)
with these rules:

- Use platform components to control mounting, `useMatchesPlatform` for
  boolean decisions, and `usePlatform` when React code needs the snapshot.
  Keep hooks unconditional and place native-only hooks in gated children.
- Outside React, pass the snapshot explicitly and use the shared matcher.
  Do not duplicate Tauri or user-agent detection in pages, hooks, or business
  helpers. Use an adapter to select native vs. browser implementations.
- Desktop OS shortcuts require the desktop surface. Use `clientOs` for
  OS-specific browser behavior. Use the appropriate viewport, package,
  path, or feature API for other decisions. Platform matching is not a
  permission check.

Update [Frontend package boundaries](../development/frontend-packages.md) with
the React-free root, the React subpath, and the app's responsibility for
composition. Add snapshot and transport injection examples to the
[frontend testing guide](../frontend-testing.md). Keep Tauri global stubs
for tests of the actual bridge. The existing [RPC guide](../development/rpc.md)
continues to own transport and authorization rules.
