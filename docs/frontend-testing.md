# Frontend tests

Frontend tests use Vitest. Pure logic runs in Node; React component and hook
integration tests run in Chromium through Vitest Browser Mode. Tests import source
through frontend workspace package exports and do not require a running Tauri
application, downloaded sidecars, or a package build. There is no interface `dist`
prerequisite.

## Setup and commands

```sh
pnpm install --frozen-lockfile
pnpm exec playwright install chromium
# On Linux, install Chromium's system dependencies too:
pnpm exec playwright install --with-deps chromium

pnpm test:frontend
pnpm test:frontend --project unit
pnpm test:frontend --project browser
pnpm test:frontend proxy-delay-history
pnpm exec vitest                         # Watch all frontend tests
pnpm exec vitest --project browser --browser.headless=false
pnpm lint:ts:tests
```

`pnpm test` runs frontend, Rust, and Deno tests once. It also requires the existing
backend build prerequisites. `pnpm typecheck` includes the frontend test typecheck;
Vitest's TS/TSX transformation alone does not check types.

## Adding tests

Place tests under the owning package's `frontend/<package>/tests/` directory:

- `*.test.ts` or `*.test.tsx`: Node unit tests.
- `*.browser.test.ts` or `*.browser.test.tsx`: browser component/hook tests.

Both projects discover nested directories automatically. Do not add a pnpm command
for individual test files. Keep tests under the owning package's `tests/`, outside
`src/`, so package typechecks and the application's route generation stay scoped
to production source.

Import `test`, `expect`, and `vi` explicitly from `vitest`. Use typed fixtures and
keep production source independent of test globals. Runtime malformed-data cases
may deliberately cross a type boundary; document the narrow assertion at that
boundary rather than disabling typechecking for the suite.

The standalone `vitest.config.ts` deliberately avoids the application's build
plugins. Package imports resolve through package exports. The test config keeps
the app `@` alias, repository-root `@root` alias, and `~icons` test stub; do not add
aliases to sibling package source or `node_modules` internals. If a test needs an
app alias, keep it consistent with `tsconfig.test.json`.
React browser tests use `vitest-browser-react` from the root test runtime. Select
one package independently with `pnpm test:frontend frontend/query/tests`, for
example. Package production dependencies remain declared in their own manifests.

## Isolation and asynchronous behavior

Test pure functions with ordinary values. For hooks, use real React and React Query
with a fresh QueryClient for each test. Mock infrastructure at the IPC boundary;
do not replace mutation lifecycle callbacks or query-cache behavior under test.
Unexpected IPC commands should fail the test.

`mockIPC` installs only `__TAURI_INTERNALS__`, while the RPC client detects the
desktop app with `isTauri()`, which reads `globalThis.isTauri`. A test that serves
commands through `mockIPC` must also `vi.stubGlobal('isTauri', true)` and restore it
with `vi.unstubAllGlobals()`; set it in `vi.hoisted` when a module selects its
transport at import time. When changing how the frontend detects its environment,
search the tests that simulate the old signal and update them in the same change.

Do not hard-code product data that is expected to change, such as presets or
default values. Take the value from its source of truth or have the test supply its
own data, so a product change does not surface as an unrelated locator timeout.

Use asynchronous assertions or explicit completion promises instead of sleeps.
Disable unrelated retries, control refetch responses, unmount components and clear
query caches after each test. Restore mocks and timers even on failure. The delay
history suite freezes only interval timers to prevent polling from overwriting the
asserted cache on slow workers, and verifies that the mutation clears its interval.

## Real HTTP UI integration

Browser Mode tests mock transports and do not verify the real backend chain. The
existing `deno task test:http-ui <running debug HTTP server URL>` checks the
browser UI and actual HTTP RPC against an already-running debug server. Use its
authenticated entry URL; the script verifies credential removal from the URL,
HTTP capability restrictions, structured errors, and absence of native API errors.
This requires a runnable backend and isolated test data. Unit tests, Browser Mode,
and a successful frontend build do not establish desktop or real HTTP acceptance.
