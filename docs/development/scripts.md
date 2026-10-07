# Repository scripts

Repository automation uses Deno TypeScript. Run public script entrypoints from
the repository root through `deno task <name>`; list the available commands with
`deno task`. The root `deno.jsonc` is the only task catalog. It delegates runtime
configuration and dependency locking to `scripts/deno.jsonc` and
`scripts/deno.lock`, keeping Deno's installed dependencies inside `scripts/`
separate from the pnpm workspace.

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
| `generate/`            | Git metadata, data slots and offline map generation                                 |
| `testing/`             | Browser integration checks requiring a running application                          |
| `frontend/`            | Frontend workspace package-boundary inventory, policy checks and gate CLI           |
| `shared/`              | Small modules reused across script categories, such as logging and repository paths |

Keep Deno configuration, its lockfile, script documentation and editor configuration
at `scripts/`; non-source fixtures belong in `scripts/fixtures/`. Co-locate
`*_test.ts` files with their domain modules. The upstream `backend/nyanpasu-runtime`
submodule owns its own tooling and is outside this layout.

Split mixed responsibilities into concrete modules: argument parsing and command
orchestration, pure computation, and infrastructure operations. Imports of library
modules must not start downloads, launch processes or run a CLI. Keep dependencies
and paths explicit. Do not add generic frameworks, broad `utils/` buckets, old-path
wrappers or compatibility exports merely to avoid updating callers.

## Task entrypoints

CI, package scripts, hooks, current documentation and application tests launching
repository tooling must use named Deno tasks. Keep existing pnpm commands as thin
`deno task` delegates where they remain useful; do not duplicate execution logic.
Do not introduce direct `deno run`, `node`, `tsx` or file-path-based script calls
outside the task catalog. Calls to external tools such as Git, tar, rustc or the
Tauri signing CLI remain implementation details inside scripts. Script unit tests
may launch a specific implementation with an injected temporary working directory
to test filesystem behavior without touching the checkout; these are not public
entrypoints. Historical plans and reports retain the commands they originally ran.

Name tasks after the operation, using colon-separated groups already used by the
package scripts (`prepare:check`, `generate:git-info`, `test:scripts`). Declare a
task for every public CLI and a description explaining its result. Forward arguments
without an extra separator, for example:

```sh
deno task prepare:check --force --arch arm64
deno task prepare:release --linux
deno task architecture-ledger --help
deno task generate:topology-map
deno task test:http-ui <running-debug-http-server-url>
```

Tasks run from the root configuration's directory. From a subdirectory, select
that configuration explicitly (`deno task --config ../../deno.jsonc <name>`, with
the appropriate relative path), since a nearer package or runtime configuration
can shadow the root tasks. Use `shared/repo-paths.ts` for source-location-based repository
paths; never calculate the root by counting parents in each moved source file.
Operations that intentionally act on a supplied workspace must accept that path
explicitly rather than changing a shared root or process working directory.
Relative command-line paths follow the task's repository-root working directory.
Preserve arguments, environment variables, permissions and failure exit status when
reorganizing tasks. Never put credentials in the task catalog.

## Dependencies and verification

Use Deno-compatible imports (`jsr:`, `npm:`, or supported `node:` APIs). Pin new npm
imports and type declarations and commit the updated Deno lockfile. Browser tooling
uses Playwright through Deno and still requires its Chromium binary; it does not
require Node to launch the repository script. The Tauri signing CLI may still use
Node as its own runtime.

Do not pass native file system paths as glob patterns: globby treats backslashes as
escapes, so an absolute Windows path matches nothing. Glob names relative to a `cwd`
or use the exact paths. Tests that compare paths normalize separators first, and a
script change is verified on Windows CI as well as Linux.

Deno formatting and type checking cover script sources recursively; frontend
Prettier conventions do not replace the existing Deno formatter for these files.

```sh
deno task lint:deno
deno task test:scripts
deno task lint:frontend-boundaries
deno task lint:architecture-ledger
deno task lint:backend-boundaries
```

Use the narrower test tasks for the affected category when appropriate. For a
reorganization, compare tests before and after, check root-relative paths and
argument forwarding, and update all active callers. Verify migrated generators
against existing output and browser tooling against a real browser. Publishing,
uploading and notification tasks have external effects: validate their local logic
and configuration without making live releases solely to test a refactor. Record
platform-specific or environment-dependent checks that could not run.
