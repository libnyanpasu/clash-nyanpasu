# Repository scripts

Repository automation is TypeScript that runs directly on Node.js, which strips
types at load time, so there is no build step. Run public script entrypoints
from the repository root through `pnpm run <name>`; list the available commands
with `pnpm run`. The root `package.json` is the only task catalog. The scripts
are the `@nyanpasu/scripts` pnpm workspace package in `scripts/`, which owns the
script dependencies in `scripts/package.json` and the type-check configuration in
`scripts/tsconfig.json`; the shared `pnpm-lock.yaml` locks them.

## Layout and responsibilities

Put handwritten script source under `scripts/src/`, grouped by responsibility:

| Directory              | Responsibility                                                                      |
| ---------------------- | ----------------------------------------------------------------------------------- |
| `architecture-ledger/` | Architecture policy, Rust scanning, snapshots and gate/report CLI                   |
| `prepare/`             | Sidecar and resource preparation, downloading and progress display                  |
| `release/`             | Version publication, build configuration, package finalization and signing          |
| `updater/`             | Updater manifests, release channels, platform selection and release notes           |
| `manifest/`            | Core version resolution and version manifest generation                             |
| `artifacts/`           | Build cache and artifact/file-server operations                                     |
| `notifications/`       | Release notifications                                                               |
| `generate/`            | Git metadata, data slots, offline map and open source credits generation            |
| `testing/`             | Browser integration checks requiring a running application                          |
| `benchmark/`           | Alternating A/B runs of the frontend page benchmarks, and profile and trace reports |
| `frontend/`            | Frontend workspace package-boundary inventory, policy checks and gate CLI           |
| `ci/`                  | Pull request change classification for CI                                           |
| `shared/`              | Small modules reused across script categories, such as logging and repository paths |

Keep `package.json`, `tsconfig.json` and the script documentation at `scripts/`; non-source fixtures belong in `scripts/fixtures/`. Co-locate
`*_test.ts` files with their domain modules. The upstream `backend/nyanpasu-runtime`
and `backend/enable-loopback` submodules own their tooling outside this layout.

Split mixed responsibilities into concrete modules: argument parsing and command
orchestration, pure computation, and infrastructure operations. Imports of library
modules must not start downloads, launch processes or run a CLI. Keep dependencies
and paths explicit. Do not add generic frameworks, broad `utils/` buckets, old-path
wrappers or compatibility exports merely to avoid updating callers.

## Task entrypoints

CI, hooks, current documentation and application tests launching repository
tooling must use named tasks from the root `package.json`, run as
`pnpm run <name>` (never bare `pnpm <name>`, because names such as `publish`
collide with pnpm built-ins). A task is `node scripts/src/<category>/<file>.ts`;
keep the execution logic in the script and do not duplicate it in the task. Do
not call `tsx` or other file-path-based runners outside the task catalog. Where a
program cannot depend on pnpm being on `PATH` (for example a Windows `.cmd` shim
under a process spawner), launch the task with `node --run <name> -- <args>`.
Calls to external tools such as Git, tar, rustc or the Tauri signing CLI remain
implementation details inside scripts. Script unit tests may launch a specific
implementation with an injected temporary working directory to test filesystem
behavior without touching the checkout; these are not public entrypoints.
Historical plans and reports retain the commands they originally ran.

Name tasks after the operation, using colon-separated groups already used by the
package scripts (`prepare:check`, `generate:git-info`, `test:scripts`). Declare a
task for every public CLI and describe its result in the task table in
`scripts/README.md`. `pnpm run` forwards arguments without an extra separator,
for example:

```sh
pnpm run prepare:check --force --arch arm64
pnpm run prepare:release --linux
pnpm run architecture-ledger --help
pnpm run generate:topology-map
pnpm run test:http-ui <running-debug-http-server-url>
```

`pnpm run` finds the root `package.json` from any subdirectory, but scripts
resolve relative command-line paths against the invocation directory, so run
them from the repository root. Use `shared/repo-paths.ts` for source-location-based repository
paths; never calculate the root by counting parents in each moved source file.
Operations that intentionally act on a supplied workspace must accept that path
explicitly rather than changing a shared root or process working directory.
Relative command-line paths follow the task's repository-root working directory.
Preserve arguments, environment variables, permissions and failure exit status when
reorganizing tasks. Never put credentials in the task catalog.

## Dependencies and verification

Use `node:` APIs and npm packages declared in `scripts/package.json`. Pin each
dependency and its `@types/*` package to an exact version and commit the updated
`pnpm-lock.yaml`. Import local modules with their `.ts` extension, and write only
erasable TypeScript syntax (no enums, namespaces or constructor parameter
properties), which `erasableSyntaxOnly` enforces. Tests use `node:test` and
`node:assert/strict`, co-located as `*_test.ts`. Browser tooling uses Playwright
and still requires its Chromium binary. The Tauri signing CLI runs through Node
as well.

Do not pass native file system paths as glob patterns: globby treats backslashes as
escapes, so an absolute Windows path matches nothing. Glob names relative to a `cwd`
or use the exact paths. Tests that compare paths normalize separators first, and a
script change is verified on Windows CI as well as Linux.

Prettier formats the script sources with the repository configuration, and
`tsc --noEmit` against `scripts/tsconfig.json` type-checks them.

```sh
pnpm run lint:ts:scripts
pnpm run lint:prettier
pnpm run test:scripts
pnpm run lint:frontend-boundaries
pnpm run lint:architecture-ledger
pnpm run lint:backend-boundaries
```

Use the narrower test tasks for the affected category when appropriate. For a
reorganization, compare tests before and after, check root-relative paths and
argument forwarding, and update all active callers. Verify migrated generators
against existing output and browser tooling against a real browser. Publishing,
uploading and notification tasks have external effects: validate their local logic
and configuration without making live releases solely to test a refactor. Record
platform-specific or environment-dependent checks that could not run.
