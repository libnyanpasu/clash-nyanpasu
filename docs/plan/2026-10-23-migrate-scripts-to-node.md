# Migrate repository scripts from Deno to Node.js

**Date:** 2026-10-23

**Status:** Planned. Branch `refactor/migrate-scripts-to-node`.

**Baseline:** `main@cf9269a3c`.

## Why

Cloudflare announced its acquisition of Deno on 2026-10-09. The Deno runtime
receives bug-fix and security releases for one more year, after which Deno-led
development ends. Repository automation must not depend on a runtime with a
fixed end of maintenance.

Node.js is chosen over Bun because it adds no new runtime: `package.json`
already pins `engines.node` 24.22.1, CI already installs Node and pnpm in most
jobs, and pnpm can own the script dependencies. Moving the whole project to Bun
was rejected: Vite, vitest browser mode and the Tauri CLI keep their current
runtime either way, while replacing pnpm would mean rebuilding the lockfile and
the workspace settings.

## Goal and scope

When this plan is done:

- Every repository script under `scripts/src/` runs on Node.js 24 with native
  TypeScript type stripping. No `tsx`, `ts-node` or bundler.
- Nothing in the active repository references Deno: no `deno.jsonc`,
  `deno.lock`, `Deno.*` API, `jsr:`/`npm:` inline specifier, `@std/*` import,
  `deno task` call, `setup-deno` step or Deno editor setting.
- Every former `deno task <name>` is available as `pnpm run <name>` with the
  same name, arguments, environment variables and exit status.

Out of scope:

- Changing what any script does. This is a runtime port, not a refactor of
  script logic. Do not rename tasks, restructure categories or "improve"
  modules beyond what the port needs.
- Historical documents under `docs/plan`, `docs/spec`, `docs/design`,
  `docs/audit`, `docs/report`, `docs/review`, `docs/probe`, `docs/roadmap`,
  `CHANGELOG.md` and `UPDATELOG.md`. They keep the commands they originally ran.
- The `backend/nyanpasu-runtime` and `backend/enable-loopback` submodules.
- `.serena/project.yml` (its Deno mention is a commented language list) and
  `LICENSE` ("denominated").

## Verified current state

| Fact                                                                                                                                                                                                                                                                                                                                                                                                                  | Source                               |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `scripts/src/` holds 120 `.ts` files, about 19,900 lines                                                                                                                                                                                                                                                                                                                                                              | `wc -l`                              |
| `deno.jsonc` is the only task catalog: 60 tasks, each with a `description`                                                                                                                                                                                                                                                                                                                                            | `deno.jsonc`                         |
| `scripts/deno.jsonc` sets `nodeModulesDir: auto` and `allowScripts` for `bufferutil@4.1.0` and `utf-8-validate@5.0.10`; `scripts/deno.lock` locks the dependencies                                                                                                                                                                                                                                                    | `scripts/`                           |
| Deno API call counts: `Deno.test` 211, `writeTextFile` 91, `readTextFile` 68, `env.get` 63, `remove` 56, `Command` 44, `makeTempDir` 33, `args` 31, `exit` 30, `mkdir` 28, `stat` 17, `build.os` 15, and a long tail (`readFile`, `makeTempFile`, `cwd`, `readDir`, `rename`, `open`, `errors.NotFound`, `chmod`, …)                                                                                                  | `grep`                               |
| `Deno.dlopen("kernel32.dll")` is used only by `benchmark/cpu-cores.ts` (Windows CPU affinity), consumed by `benchmark/bench-ab.ts`                                                                                                                                                                                                                                                                                    | `grep`                               |
| `Deno.serve` is used only in `artifacts/internet-archive-upload_test.ts` (3 local fixture servers)                                                                                                                                                                                                                                                                                                                    | `grep`                               |
| `Deno.execPath()` spawns a child script in 3 tests: `internet-archive-upload_test.ts`, `windows-bundle_test.ts`, `sign-nightly-updater_test.ts`                                                                                                                                                                                                                                                                       | `grep`                               |
| `import.meta.main` is used 32 times, `import.meta.url` 8 times                                                                                                                                                                                                                                                                                                                                                        | `grep`                               |
| No `enum`, `namespace` or constructor parameter properties exist, so Node's type stripping runs the sources without transformation                                                                                                                                                                                                                                                                                    | `grep`                               |
| Imports: `jsr:@std/path` 54, `jsr:@std/assert` 41, `jsr:@std/cli/parse-args` 14, `jsr:@std/fs` 9, plus `@std/toml`, `@std/fmt/bytes`, `@std/async/retry`, `@std/io/write-all`, `@std/testing/bdd`; npm: `globby`, `semver`, `adm-zip`, `octokit`, `zod`, `prettier`, `playwright`, `grammy`, `figlet`, `es-toolkit`, `d3`, `consola`, `picocolors`, `colorize-template`, `spdx-license-list`, `spdx-expression-parse` | `grep`                               |
| Scripts are formatted by `deno fmt` (double quotes, semicolons); `scripts/` is in `.prettierignore`. The repository Prettier config uses single quotes and no semicolons                                                                                                                                                                                                                                              | `.prettierignore`, `.prettierrc.cjs` |
| 13 workflows use `denoland/setup-deno@v2` (21 steps) and call `deno task` 36 times. `deps-message-telegram`, `deps-publish-storage`, `sourceforge-backfill-release`, `storage-recovery`, `target-dev-build` and `target-release-build` install Deno but neither Node nor pnpm                                                                                                                                         | `.github/workflows/`                 |
| Root `package.json` scripts already delegate to `deno task` for about 25 tasks; `beforeBuildCommand` in `backend/tauri/tauri.conf.json` calls `pnpm generate:credits` and `pnpm run-p … generate:git-info`                                                                                                                                                                                                            | `package.json`, Tauri conf           |
| `.lintstagedrc.js` runs `deno task fmt:scripts-files`, `check:scripts-files` and `lint:frontend-boundaries`                                                                                                                                                                                                                                                                                                           | `.lintstagedrc.js`                   |
| A Rust test in `backend/tauri/src/unified_rpc.rs` (~line 1184) launches `deno task test:http-ui <url>`                                                                                                                                                                                                                                                                                                                | `unified_rpc.rs`                     |
| `pnpm publish` is a pnpm built-in, so `pnpm publish` does not run the `publish` script                                                                                                                                                                                                                                                                                                                                | pnpm CLI                             |

Files outside `scripts/` that mention Deno and must change: `.github/workflows/*`
(13 files), `.gitignore`, `.lintstagedrc.js`, `.oxlintrc.json`, `.prettierignore`,
`.vscode/extensions.json`, `AGENTS.md`, `CONTRIBUTING.md`, `README.md`,
`backend/tauri/src/unified_rpc.rs`, `credits/about.toml`, `credits/components.toml`,
`deno.jsonc`, `docs/development/{README,architecture,backend-packages,frontend-packages,scripts,testing,workflow}.md`,
`docs/frontend-testing.md`, `docs/testing/windows-bundle.md`,
`frontend/nyanpasu/perf/README.md`, `frontend/nyanpasu/src/assets/maps/README.md`,
`frontend/nyanpasu/src/generated/data-slots.gen.ts` (regenerate, do not hand-edit),
`frontend/nyanpasu/src/pages/(main)/main/settings/about/credits/_modules/use-open-source-credits.ts`,
`package.json`, `scripts/.vscode/settings.json`, `scripts/README.md`. Re-run the
search before finishing; this list is a starting point, not a guarantee.

## Design decisions

These are decided. Do not reopen them during implementation; if one turns out
to be wrong, stop and report instead of improvising.

### Runtime and TypeScript

- Run sources directly: `node scripts/src/<path>.ts`. Node 24 strips types by
  default. Keep the explicit `.ts` extensions in relative imports (Deno already
  requires them).
- Add `scripts/tsconfig.json` for type checking only: `noEmit`,
  `allowImportingTsExtensions`, `erasableSyntaxOnly`, `verbatimModuleSyntax`,
  `module`/`moduleResolution` `nodenext`, `types: ["node"]`, extending
  `tsconfig.base.json` where its options do not conflict. `verbatimModuleSyntax`
  forces `import type` for type-only imports, which type stripping requires.
- Keep `import.meta.main` (supported since Node 24.2). Replace
  `import.meta.url`-based path math with `import.meta.dirname`/`filename` only
  where `shared/repo-paths.ts` does not already cover it.

### Package and dependencies

- Make `scripts/` a private pnpm workspace package `@nyanpasu/scripts` with its
  own `package.json` (`"type": "module"`), and add `scripts` to
  `pnpm-workspace.yaml`. This preserves today's separation between script
  dependencies and frontend dependencies, with one lockfile.
- Pin every dependency to the exact version currently resolved in
  `scripts/deno.lock`. Do not upgrade anything in this change.
- Move the `allowScripts` entries to pnpm's existing `onlyBuiltDependencies`/
  `allowBuilds` in `pnpm-workspace.yaml`, and only if a dependency still needs
  its install script.

### Standard library replacements

| Deno / `@std`                                                                                                                                                                         | Node replacement                                                                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Deno.readTextFile`, `writeTextFile`, `readFile`, `writeFile`, `mkdir`, `remove`, `stat`, `rename`, `copyFile`, `chmod`, `realPath`, `readDir`, `makeTempDir`, `makeTempFile`, `open` | `node:fs/promises` (`rm` with `recursive`/`force` where `remove` used `recursive`; `mkdtemp(join(tmpdir(), prefix))`)                                                                                                                                 |
| `Deno.errors.NotFound`                                                                                                                                                                | `error.code === 'ENOENT'`, via one small type guard in `shared/`                                                                                                                                                                                      |
| `Deno.env.get/set/delete`                                                                                                                                                             | `process.env`                                                                                                                                                                                                                                         |
| `Deno.args`, `exit`, `exitCode`, `cwd`, `build.os`, `build.arch`, `execPath`, `stdout`                                                                                                | `process.argv.slice(2)`, `process.exit`, `process.exitCode`, `process.cwd()`, `process.platform` (map `win32`→`windows` where code compares Deno names), `process.arch` (map `x64`→`x86_64`, `arm64`→`aarch64`), `process.execPath`, `process.stdout` |
| `Deno.Command`                                                                                                                                                                        | `node:child_process` (`spawn`/`execFile`). Add one `shared/command.ts` with the minimal surface the call sites need (run to completion with captured or inherited output, and a spawned child handle). No generic framework                           |
| `Deno.serve` (tests)                                                                                                                                                                  | `node:http` `createServer` on port 0                                                                                                                                                                                                                  |
| `Deno.dlopen` (benchmark)                                                                                                                                                             | `koffi`, pinned, used only by `benchmark/cpu-cores.ts`                                                                                                                                                                                                |
| `Deno.test`                                                                                                                                                                           | `node:test` (`test`, `describe`, `it`; `ignore` → `skip`)                                                                                                                                                                                             |
| `@std/assert`                                                                                                                                                                         | `node:assert/strict` (`assertEquals` → `deepStrictEqual`, `assertRejects` → `rejects`, …)                                                                                                                                                             |
| `@std/testing/bdd`                                                                                                                                                                    | `node:test` `describe`/`it`                                                                                                                                                                                                                           |
| `@std/path`                                                                                                                                                                           | `node:path` (`fromFileUrl` → `node:url` `fileURLToPath`)                                                                                                                                                                                              |
| `@std/fs`                                                                                                                                                                             | `node:fs/promises` (`ensureDir` → `mkdir({ recursive: true })`, `exists` → `access`/`stat` with `ENOENT`, `walk`/`expandGlob` → existing `globby`, `copy` → `cp({ recursive: true })`)                                                                |
| `@std/cli/parse-args`                                                                                                                                                                 | `node:util` `parseArgs`. Preserve each CLI's accepted flags, aliases, defaults and unknown-flag behavior exactly; add a test where a CLI's behavior is not already covered                                                                            |
| `@std/toml`                                                                                                                                                                           | `smol-toml`                                                                                                                                                                                                                                           |
| `@std/fmt/bytes`                                                                                                                                                                      | A local `formatBytes` in `shared/`, matching the current output                                                                                                                                                                                       |
| `@std/async/retry`                                                                                                                                                                    | A local `retry` in `shared/`, keeping the current attempt count and backoff                                                                                                                                                                           |
| `@std/io/write-all`                                                                                                                                                                   | `stream.write` with backpressure (`once(stream, 'drain')`)                                                                                                                                                                                            |

### Task catalog

- The root `package.json` `scripts` field becomes the only task catalog. Each
  former Deno task keeps its exact name and runs
  `node scripts/src/<path>.ts [fixed args]`. Existing `"x": "deno task x"`
  delegates are replaced in place, not duplicated.
- Task descriptions move into a table in `scripts/README.md` (name → result).
- CI, hooks, documentation and the Rust test call tasks as `pnpm run <name>`
  (always with `run`, so built-ins such as `pnpm publish` never shadow a task).
  Arguments are forwarded without an extra `--` separator, as today.
- `test:scripts` runs `node --test "scripts/src/**/*_test.ts"`; the narrower
  test tasks pass their explicit file lists. Tests that need permission-free
  behavior differences do not exist, so `-A` is simply dropped.
- `lint:deno`, `fmt:scripts`, `check:scripts`, `fmt:scripts-files` and
  `check:scripts-files` are removed. Scripts are formatted by the repository
  Prettier and linted by oxlint like other TypeScript, and type-checked by a new
  `lint:ts:scripts` (`tsc --noEmit --project scripts/tsconfig.json`) added to
  `lint:ts`. `fmt:deno` in `package.json` is removed.
- The Rust test launches `node --run test:http-ui -- <url>` instead of `deno`,
  because `node` is a real executable on Windows while `pnpm` is a `.cmd` shim
  that `tokio::process::Command` does not resolve.

### CI

- Replace each `denoland/setup-deno@v2` step. Jobs that already set up Node and
  pnpm only need the step removed and `deno task x` changed to `pnpm run x`.
- The six jobs that have neither Node nor pnpm get the same
  `pnpm/action-setup` + `actions/setup-node` steps used elsewhere in the
  repository, then `pnpm install --frozen-lockfile --filter @nyanpasu/scripts`
  so they do not install the frontend workspace.
- Keep workflow display names and job structure unchanged.

## Steps and commits

Each commit builds, passes its checks and does one thing.

1. **Port script sources to Node APIs while still running on Deno.**
   Deno executes `node:` modules and npm packages, so the source port can be
   verified with the existing Deno test runner before the runtime flips. Work
   one category directory at a time (`shared/` first), replacing `Deno.*` and
   `@std/*` per the tables above, except `Deno.test`, `@std/assert` and
   `@std/testing`, which stay until step 2. One commit per category or per
   small group of categories, e.g.
   `refactor(scripts): use Node APIs in release scripts`.
   - verify after each commit: `deno task test:scripts` and `deno task lint:deno`
     pass with the same test count as the baseline.

2. **Switch the runtime to Node.js.** One commit,
   `refactor(scripts): run repository scripts on Node.js`:
   - add `scripts/package.json`, `scripts/tsconfig.json`, the workspace entry
     and the lockfile update;
   - convert tests to `node:test` and `node:assert/strict`;
   - rewrite the task catalog in the root `package.json`, update
     `.lintstagedrc.js`, `.prettierignore`, `.oxlintrc.json`, `.gitignore`,
     `.vscode/extensions.json`, the Rust test, all CI workflows, and every
     non-historical document and code comment listed above, including
     `AGENTS.md` and `docs/development/scripts.md` (rewrite the guide for Node:
     `pnpm run`, `node:` APIs, `node:test`, Prettier, `scripts/package.json`);
   - regenerate `data-slots.gen.ts` with the new command so its header updates;
   - delete `deno.jsonc`, `scripts/deno.jsonc`, `scripts/deno.lock` and
     `scripts/.vscode/`.
   - verify: see "Verification" below.

3. **Reformat scripts with Prettier.** One mechanical commit,
   `style(scripts): format repository scripts with Prettier`, containing only
   `prettier --write scripts` output and no other change, so review of step 2
   is not buried under formatting.

## Verification

Record the baseline before step 1 (test count from `deno task test:scripts`,
and the outputs listed below), then confirm after step 3:

- `pnpm run test:scripts` passes with the same number of tests as the Deno
  baseline (count tests, not files).
- `pnpm run lint:ts`, `pnpm run lint:prettier`, `pnpm run lint:oxlint` pass.
- `pnpm run lint:architecture-ledger`, `pnpm run lint:backend-boundaries` and
  `pnpm run lint:frontend-boundaries` pass and print the same result as on the
  baseline.
- Generators produce byte-identical output to the baseline:
  `generate:git-info`, `generate:data-slots` (except its header line),
  `generate:credits`, `generate:manifest` with a fixed input where possible,
  and the topology/offline map generator.
- `pnpm run prepare:check --force` downloads and verifies sidecars on macOS.
- Every task in the catalog at least starts and prints help or fails on missing
  credentials the same way as before. Never run publishing, uploading,
  notification or cache-upload tasks for real.
- `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu
<http-ui test name>` passes with the new `node --run` launch, if a local
  Playwright Chromium is available; otherwise record that it could not run.
- `git grep -i deno` returns only historical documents, the submodules,
  `LICENSE`, `.serena/project.yml` and unrelated words.
- Windows behavior (path globbing, `.cmd` resolution, `dlopen` replacement)
  cannot be checked locally on macOS; it is verified by the Windows CI jobs on
  the pull request. Note this in the PR description.

## Risks

- **CLI argument parsing drift.** `@std/cli` and `node:util` `parseArgs` differ
  (booleans, negation, unknown flags, collect). Compare each CLI's flags before
  and after and test the ones without coverage.
- **Platform name mapping.** `Deno.build.os`/`arch` and `process.platform`/`arch`
  use different strings; a missed mapping silently picks the wrong sidecar.
- **Stream and process semantics.** `Deno.Command` output collection and exit
  codes must be reproduced exactly, including non-zero exits that scripts
  currently inspect rather than throw on.
- **CI-only paths.** Release and nightly workflows only run on tags and
  schedules. Review their diff line by line, since pull request CI does not
  exercise them.
