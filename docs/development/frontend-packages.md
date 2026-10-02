# Frontend package boundaries

The frontend workspace has nine private source packages:

| Package               | Owns                                                         | Dependency boundary                                       |
| --------------------- | ------------------------------------------------------------ | --------------------------------------------------------- |
| `@nyanpasu/constants` | Immutable values shared across packages                      | No app imports or runtime services                        |
| `@nyanpasu/utils`     | Pure reusable helpers such as `cn` and `chains`              | No React, Tauri, or app imports                           |
| `@nyanpasu/hooks`     | Browser and React hooks, including shared breakpoints        | Depends on constants; no app imports                      |
| `@nyanpasu/platform`  | Platform capabilities and browser/native adapters            | Native access is provided lazily at the boundary          |
| `@nyanpasu/rpc`       | Generated operation bindings and transport adapters          | No React Query or React dependency                        |
| `@nyanpasu/query`     | Query/mutation hooks, providers, and application data access | Depends on RPC; receive the RPC client/context explicitly |
| `@nyanpasu/theme`     | Theme computation and shared theme assets                    | No app or route dependencies                              |
| `@nyanpasu/ui`        | Reusable presentational controls and primitives              | No app, platform, RPC, or query imports                   |
| `@nyanpasu/nyanpasu`  | Desktop/browser application composition                      | Owns routes, providers, Vite plugins, and Paraglide       |

Dependencies flow from the app into these capabilities, not back into the app.
`rpc` is a transport layer; `query` adds React Query behavior above it. UI
components receive labels, locale-sensitive strings, and callbacks through props.
Platform-dependent features use an injected adapter or lazy factory so importing a
shared package does not eagerly access Tauri. Providers receive callbacks such as
`onDegraded` as parameters rather than reading global application state. Avoid
cycles between packages.

Every shared package is private, exposes its public API through `src/index.ts`,
and typechecks with `noEmit`. Root imports use package names; subpath imports
remain available for individual modules and lazy loading. The UI package maps
component subpaths with a wildcard instead of listing every component. Do not create an `interface` distribution
package or require generated `dist` files before app builds. Consumers import the
package API (`import { Button, Card } from '@nyanpasu/ui'`, for example), not a sibling package's `src`
path. The app may use its own `@` alias; shared-package imports resolve through
workspace package exports.

The root `tsconfig.json` has `files: []` and references each package as an index.
It is not a `tsc -b` publisher or build pipeline. The base, browser, and Node
configs share compiler settings; each frontend package includes only its own
`src`. In particular, the app config must not include sibling package sources.
`pnpm typecheck` runs each frontend package's `typecheck` script, followed by the
Vite/Node configuration, performance, and test TypeScript projects. Tests live under
`frontend/<package>/tests/`, outside `src`; see the [Vitest guide](../frontend-testing.md).

The app is the only owner of route generation and Paraglide. Its Vite config
composes app plugins; importing a package does not make that package responsible
for route or locale generation. The app Tailwind entry scans shared packages that
produce UI classes (currently `frontend/ui/src`) so their styles are included. The
data-slot generator scans `frontend/*/src/**/*.tsx` and writes the generated slot
inventory into the app. Preserve slots as a styling contract when moving
components.

The standalone test Vite config avoids app build plugins. It uses the app `@`
alias, repository-root `@root` alias, and a `~icons` test stub. Other package
imports use package exports; do not add aliases into sibling package source or
`node_modules` internals.

`deno task lint:frontend-boundaries` checks source exports, declared workspace
imports, dependency cycles, reverse app dependencies, and package responsibilities.
It also rejects reintroducing the retired `interface` package. The root
`lint:frontend-boundaries` script includes this gate in the normal lint pipeline.
