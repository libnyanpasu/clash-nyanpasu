# Frontend tests

Frontend tests use Vitest. Pure logic runs in Node; React component and hook
integration tests run in Chromium through Vitest Browser Mode. Tests import source
directly and do not require a running Tauri application, downloaded sidecars, or a
frontend build.

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
for individual test files. Keep tests outside `src/` so they are excluded from
package declaration builds and the application's route generation.

Import `test`, `expect`, and `vi` explicitly from `vitest`. Use typed fixtures and
keep production source independent of test globals. Runtime malformed-data cases
may deliberately cross a type boundary; document the narrow assertion at that
boundary rather than disabling typechecking for the suite.

The standalone `vitest.config.ts` deliberately avoids the application's build
plugins. Use relative source imports; if a new test needs an application alias,
configure it in both the test runtime and `tsconfig.test.json`, resolving it from
the configuration file. Do not depend on a previously built interface `dist`.
React browser tests use `vitest-browser-react`; declare it in the consuming package
when adding browser tests to another package.

## Isolation and asynchronous behavior

Test pure functions with ordinary values. For hooks, use real React and React Query
with a fresh QueryClient for each test. Mock infrastructure at the IPC boundary;
do not replace mutation lifecycle callbacks or query-cache behavior under test.
Unexpected IPC commands should fail the test.

Use asynchronous assertions or explicit completion promises instead of sleeps.
Disable unrelated retries, control refetch responses, unmount components and clear
query caches after each test. Restore mocks and timers even on failure. The delay
history suite freezes only interval timers to prevent polling from overwriting the
asserted cache on slow workers, and verifies that the mutation clears its interval.

## TODO: WebUI end-to-end tests

Browser Mode tests currently mock IPC and do not verify the real backend chain.
Real E2E is deferred until IPC is decoupled and an independent WebUI exists. At that
point, evaluate Playwright Test against the running WebUI and its actual service,
with isolated data, readiness checks and deterministic teardown. No Tauri desktop
driver or placeholder E2E command is introduced by this migration.

See the [migration plan](superpowers/plans/2026-09-16-frontend-vitest-migration.md)
for the deferred prerequisites and acceptance criteria.
