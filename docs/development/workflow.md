# Development workflow

## Scope and verification

State assumptions and discuss ambiguous requirements before implementation. Keep
changes focused on the requested behavior, match existing style, and remove only
dead code created by your change. Prefer the smallest complete solution; this does
not justify hidden global dependencies.

Define observable success criteria before editing. For bugs, reproduce the failure
and verify the fix. For refactors, verify behavior before and after. For multi-step
work, pair each step with a check. Explain material tradeoffs and blockers rather
than preserving an undocumented compatibility layer.

Do not merge a pull request or push to `main` while a CI check is red, even when
only one platform fails; a Windows-only failure is a failure, not platform noise.
When `main` turns red, fix it before landing further changes. Failures that pile up
on a red branch are hard to separate, and an earlier failure can hide a later one.

## Repository scripts

Follow [Repository scripts](scripts.md) for the source layout, Deno configuration
and named task entrypoints. CI and package scripts invoke repository tooling
through `deno task`; keep its execution logic in the root task catalog.

## GitHub workflows

- Use `[Category] Action Object` for workflow display names, with the categories `CI`, `Release`, `Maintenance`, and `Reusable`.
- Use `Reusable` for workflows exposed through `workflow_call`, even when they also support manual dispatch. Other categories describe the entry workflow's purpose.
- Name the actual operation and output: distinguish nightly publication, release package publication, draft release preparation, core version manifests, and app updater manifests. Avoid scope labels such as `Entire` and `Single`.
- Preserve workflow file paths and CI job names during display-name cleanup; review callers, badges, documentation, and required checks before renaming those identifiers.
- Remove workflows only after checking reusable callers and automatic/manual entry points; lack of recent runs alone does not prove a workflow is unused.
- Separate adjacent workflow steps with one blank line, keeping each step's explanatory comments after the separator.

The repository currently keeps all 17 workflows. Reusable workflows have callers or a manual
entry point. The central-storage publisher is called by nightly and release workflows;
the SourceForge release backfill is a manual entry point. Release publication and
SourceForge backfills share one concurrency group so a backfill cannot race a newly
published release. Central storage first prepares the complete six-target inventory;
SourceForge and Telegram then run as independent jobs with the original
publication timestamp. Normal publication, recovery and backfill writers use the
backend-specific `storage-publication-sourceforge` or `storage-publication-telegram`
lock. GitHub updater generation runs directly after release asset upload, independently
of both storage jobs. A second updater job attaches verified SourceForge metadata
only after the base updater and mirror verification succeed. The manifest maintenance workflow updates both `main` and the still
maintained v1 `dev` branch; a failure in one branch does not make the other job
redundant. GitHub-generated Copilot workflows are managed by GitHub rather than
these YAML files.

| File under `.github/workflows/`     | Display name                                          | Purpose / entry point                                                                                                |
| ----------------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `ci.yml`                            | `[CI] Lint, Build, and Test`                          | Lint, build, and test pushes and pull requests.                                                                      |
| `daily.yml`                         | `[Maintenance] Update Core Version Manifests`         | Scheduled or manual refresh of core versions on main and v1/dev.                                                     |
| `stale.yml`                         | `[Maintenance] Close Stale Issues and Pull Requests`  | Scheduled or manual stale issue and pull request maintenance.                                                        |
| `publish.yml`                       | `[Release] Prepare Version and Draft Release`         | Manual version bump, tag, and draft release creation; publishing the draft starts package builds.                    |
| `target-dev-build.yaml`             | `[Release] Build and Publish Nightly`                 | Scheduled or manual nightly builds and publication across six OS/architecture targets.                               |
| `target-release-build.yaml`         | `[Release] Build and Publish Release Packages`        | Build and publish packages when a stable or prerelease release is published.                                         |
| `deps-build-linux.yaml`             | `[Reusable] Build Linux Packages`                     | Linux artifacts; called by nightly and release workflows, or dispatched manually.                                    |
| `deps-build-macos.yaml`             | `[Reusable] Build macOS Packages`                     | macOS artifacts; called by nightly and release workflows, or dispatched manually.                                    |
| `deps-build-windows-nsis.yaml`      | `[Reusable] Build Windows NSIS and Portable Packages` | Windows installers and optional portable artifacts; called by nightly and release workflows, or dispatched manually. |
| `deps-create-updater.yaml`          | `[Reusable] Publish Updater Manifests`                | App updater feeds on GitHub and Surge; called by nightly and release workflows, or dispatched manually.              |
| `deps-delete-releases.yaml`         | `[Reusable] Clear Release Assets`                     | Clear assets on the existing nightly release; called by nightly workflow, or dispatched manually.                    |
| `deps-message-telegram.yaml`        | `[Reusable] Notify Telegram of Releases`              | Release notifications; called by nightly and release workflows, or dispatched manually.                              |
| `deps-publish-storage.yaml`         | `[Reusable] Publish Central Storage Mirrors`          | Publish six-target build inventories to SourceForge and Telegram; called by nightly and release workflows.           |
| `deps-update-tag.yaml`              | `[Reusable] Update Nightly Tag and Release`           | Move the nightly tag and update release metadata; called by nightly workflow, or dispatched manually.                |
| `deps-upload-release-assets.yaml`   | `[Reusable] Upload Release Assets`                    | Upload artifacts from the current caller run to its release; called by nightly and release workflows.                |
| `sourceforge-backfill-release.yaml` | `[Maintenance] Backfill SourceForge Release Mirror`   | Manually mirror an existing GitHub release to SourceForge; serialized with release publication.                      |
| `storage-recovery.yaml`             | `[Maintenance] Debug and Recover Storage Publication` | Check storage or register, retransfer, and verify retained package artifacts without rebuilding.                     |

## Worktrees and resource reuse

Feature/refactoring work runs in isolated git worktrees by default. Working in the current checkout is an option when the user chooses it after a cost assessment. Before implementation:

- Consider the task's scope, expected duration, concurrent work, and existing uncommitted changes in the current checkout.
- Weigh the isolation benefits against dependency installation, independent Cargo builds, disk usage, and preparation of gitignored build prerequisites. Reuse a suitable existing worktree when available.
- Summarize the relevant costs and benefits, state a recommendation, then ask the user whether to work in a worktree or the current checkout. Wait for their choice before implementation; if they have already specified a choice for the task, follow it without asking again.
- Keep isolated worktrees as the recommended default for feature/refactoring work. Small, isolated edits may be cheaper in the current checkout; broad changes or concurrent work strengthen the case for a worktree. The assessment adds a user-selectable alternative, not a replacement for the isolation and resource reuse policy.

When the user chooses a worktree, its location is the developer's choice (any path outside the repo tree). Worktrees share the main `.git`. The rule: reuse expensive **branch-independent** assets from the main checkout via symlink, and regenerate everything **branch-dependent** per worktree.

### Reuse policy

| Path (repo-relative)       | Approx size  | Policy                          | Reason                                                                                                                                         |
| -------------------------- | ------------ | ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/sidecar/`   | ~213M        | **Symlink → main**              | gitignored downloaded cores (mihomo / clash-rs / clash / nyanpasu-service); branch-independent; re-fetch via `deno task prepare:check` is slow |
| `backend/tauri/resources/` | ~21M         | **Symlink → main**              | gitignored static assets (`geoip.dat`, `geosite.dat`, `Country.mmdb`, `wintun.dll`, service exes); branch-independent                          |
| `node_modules/`            | ~1.5G        | **Independent `pnpm install`**  | pnpm global store already hardlink-dedupes; sharing risks concurrent lock conflicts                                                            |
| `backend/target/`          | ~50G         | **Independent — never symlink** | sharing causes Cargo incremental-fingerprint churn + concurrent build-lock waits across diverged source trees                                  |
| `backend/tauri/tmp/dist/`  | build output | **Independent — never symlink** | branch-dependent frontend build; `emptyOutDir: true` means one worktree's `web:build` wipes the shared dir                                     |

Only `sidecar/` and `resources/` are symlink candidates.

### Gitignored build prerequisites a fresh worktree lacks

- **`backend/tauri/tmp/dist`** — `backend/tauri/build.rs` calls `tauri_build::build()`, which validates `frontendDist: ./tmp/dist` **at compile time**. When missing, every `cargo build` / `clippy` / `cargo test --all-features` / rust-analyzer run on the tauri crate fails. Resolve one of:
  - Rust-only worktree → drop a placeholder (cheapest, no vite build).
  - Runnable UI → `pnpm web:build` (workspace packages resolve from source; this clears and refills `tmp/dist`).

`backend/tauri/tmp/git-info.json` is optional (`build.rs` guards it with `exists()`); run `deno task generate:git-info` only if accurate commit metadata must be baked in.

### Create a worktree

Commands shown for Windows / PowerShell (dir symlinks need Developer Mode, no elevation). `<worktree-path>` and `<type>/<name>` are yours to choose.

```powershell
$main = git rev-parse --show-toplevel                 # capture main checkout root
git worktree add <worktree-path> -b <type>/<name>
cd <worktree-path>

# Reuse branch-independent downloads (symlink back to main)
New-Item -ItemType SymbolicLink backend/tauri/sidecar   -Target "$main/backend/tauri/sidecar"
New-Item -ItemType SymbolicLink backend/tauri/resources -Target "$main/backend/tauri/resources"

pnpm install

# Satisfy tauri-build's frontendDist check — pick one:
New-Item -ItemType Directory -Force backend/tauri/tmp/dist | Out-Null            # A) Rust-only placeholder
Set-Content backend/tauri/tmp/dist/index.html '<!doctype html><title>dev</title>'
# pnpm web:build                                      # B) real UI (builds app and replaces tmp/dist)
```

### Remove a worktree

`git worktree remove` on Windows can fail with `Filename too long` because per-worktree `node_modules` / `target` hold paths over MAX_PATH. Force-delete with the extended-length prefix, then reconcile git:

```powershell
Remove-Item -LiteralPath "\\?\<absolute-worktree-path>" -Recurse -Force
git worktree prune
git worktree list
```

Removal reclaims only the worktree's own files and its symlinks (pointers back to main) — it never touches the main checkout's real `sidecar/` / `resources/`.

## Git commits

### Stage only related files

Before committing, run `git status` to review the changes, stage only the files related to this change with explicit paths (`git add <specific-path>`), then verify with `git diff --cached --stat`.

Never use blanket staging such as `git add .`, `-A`, `--all`, or `*`. If something was staged by mistake, unstage it with `git reset HEAD <path>`.

### One commit does one thing

Every commit must be atomic, complete, and buildable.

- One indivisible task is one commit.
- Multiple independent tasks are split into multiple commits.
- Do not commit code you know is broken.
- Do not make fix-up (patch-style) commits on a development branch.

If a commit on a development branch is flawed and has not been pushed, fix it with `git reset --soft HEAD~1` and recommit. If it has already been pushed, any rewrite, amend, or force push requires explicit consent first.

Self-check before committing: does this change complete or correct the previous commit? If yes, fold it into the previous commit with `git reset --soft HEAD~1` and recommit instead of creating a new one. Even when two commits are each individually clean, a later commit that completes an earlier one is still a fix-up commit.

Exploratory work may live on `temp/`, `wip/`, or `scratch/` branches. Do not merge those directly; create a clean branch and reorganize the work into atomic commits.

### Commit message content

The subject states what changed; the body explains why when the problem or the fix is not obvious.

Subject rules:

- Use the imperative mood, stay within 72 characters, and do not end with a period.
- Describe the behavior or capability.

Body rules:

- A non-trivial change must have a body; the body may be omitted only when the subject is fully self-explanatory.
- Explain the root cause and the rationale for the fix: why this is a bug and why this change is needed.
- Do not enumerate changes file by file, and do not restate implementation steps that the diff already shows.
- Describe only the final state relative to the parent commit, not differences between intermediate versions of the same patch (e.g. "v2 fixes X").

### Trust the reader

Assume the reader is a competent developer familiar with the project; do not explain what they already know:

- How to build the project — that belongs in documentation, not in a commit message.
- Obvious statements of usage. Counter-example:

  ```text
  Example usage:
    # use mkv container:
    ffmpeg -hwaccel d3d12va -hwaccel_output_format d3d12 -i input.mp4 -c:v av1_d3d12va output.mkv
  ```

- "Build succeeded" or "all tests green" — the commit's existence already implies it passed.

Mention these only when they are genuinely non-obvious:

- New test commands or tools that do not yet exist in the project.
- Non-standard configuration required to reproduce the result.
- Unusual constraints that affect how the data should be interpreted.
