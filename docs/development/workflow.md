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
| `ci.yml`                            | `[CI] Lint, Build, and Test`                          | Lint, build, and test pushes; a pull request checks only the parts it changes.                                       |
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

### CI checks by changed part

`ci.yml` uses `pull_request_target: closed` to cancel a closed or merged pull
request's running or pending CI through the existing concurrency group. This
base-branch event runs only the `Cancel Closed Pull Request CI` job, with no token
permissions or checkout; `Changes` and its dependent jobs are skipped. The group
uses the pull request number rather than the merged branch ref, so closing a pull
request does not cancel the target branch's push CI. Normal pull request checks
continue to use `pull_request`.

`ci.yml` checks a pull request only in the parts it changes. Its `Changes` job
runs `deno task check:changed-parts HEAD^1` on the merge commit and outputs
`frontend`, `backend` and `scripts`. The rules are the ordered `CHANGE_RULES`
table in `scripts/src/ci/changed-parts.ts`: the first rule matching a file
decides its parts, and a file no rule matches checks every part. Pushes check
every part.

| Part       | Lint                                                                       | Unit Test                                          | Build Tauri                     |
| ---------- | -------------------------------------------------------------------------- | -------------------------------------------------- | ------------------------------- |
| any change | Ubuntu: Prettier                                                           | —                                                  | —                               |
| `frontend` | Ubuntu: oxlint, Stylelint, TypeScript, frontend boundaries                 | Ubuntu: Vitest                                     | Ubuntu: `pnpm -r build`         |
| `backend`  | every OS: Clippy, rustfmt; Ubuntu: architecture ledger, backend boundaries | every OS: `cargo test`; Ubuntu: bindings freshness | every OS: release `cargo build` |
| `scripts`  | Ubuntu: `lint:deno`, frontend and backend boundaries                       | every OS: `test:scripts`                           | —                               |

The ruleset requires every matrix check of Lint, Build Tauri and Unit Test by
name. A job skipped by a job-level `if` reports once without its matrix, so those
checks would never report and block the pull request. Gate steps on the
`Changes` outputs instead, and keep every job and matrix entry running.

When moving a directory, adding a root configuration file or changing a
framework, update `CHANGE_RULES` and its test in the same change, and keep the
step conditions in `ci.yml` and this table in sync with the checks each part
needs.

## Worktrees and resource reuse

Feature/refactoring work runs in isolated git worktrees by default. Working in the current checkout is an option when the user chooses it after a cost assessment. Before implementation:

- Consider the task's scope, expected duration, concurrent work, and existing uncommitted changes in the current checkout.
- Weigh the isolation benefits against dependency installation, independent Cargo builds, disk usage, and preparation of gitignored build prerequisites. Reuse a suitable existing worktree when available.
- Summarize the relevant costs and benefits, state a recommendation, then ask the user whether to work in a worktree or the current checkout. Wait for their choice before implementation; if they have already specified a choice for the task, follow it without asking again.
- Keep isolated worktrees as the recommended default for feature/refactoring work. Small, isolated edits may be cheaper in the current checkout; broad changes or concurrent work strengthen the case for a worktree. The assessment adds a user-selectable alternative, not a replacement for the isolation and resource reuse policy.

When the user chooses a worktree, choose a path outside the repo tree using the existing filesystem policy below; respect any location the developer has explicitly selected. Worktrees share the main `.git`. Reuse expensive **branch-independent** assets from the main checkout via symlink. Each worktree owns its **branch-dependent** outputs; its Cargo target may start as an independent copy-on-write (CoW) clone of an existing target.

### Filesystem and location policy

Inspect the filesystem containing the project and the volume containing the source target before creating a worktree. Check the proposed destination's existing parent as well: a matching filesystem type on two different volumes does not establish clone support. Prefer an existing parent outside the repo tree on the same clone-capable volume. On Windows, if the project is on ReFS, create the worktree on that same ReFS volume rather than defaulting to a worktree directory on NTFS. If the developer explicitly chooses a different location, explain any loss of cloning and use an independent target there.

Use existing filesystem capabilities only. Do not reformat or repartition disks, create or mount VHD/VHDX images, add filesystems or mount points, create snapshot datasets/subvolumes, move existing checkouts or caches, or change global environment variables or tool configuration to enable reuse. Creating a new worktree and its own target under an existing parent is the intended operation; reorganizing the developer's existing directories is not.

Before preparing the target, tell the user which filesystem and source/destination volume were detected, which native reuse method will be used, and whether that method was previously used or verified for this project and these paths. Prefer an existing applicable method rather than introducing a different one. A previous successful clone or probe may establish support while the relevant volumes, OS and copy tool remain applicable; recheck when those conditions change. This is a status explanation, not a separate approval step after the worktree choice is already authorized. If native cloning is unsupported or cannot be verified with existing tools, briefly explain that target reuse will be skipped and build independently. Do not pursue VHDs, filesystem changes or other storage workarounds.

| Existing environment                                             | Target reuse in a new worktree                                 | Conditions                                                                                                                         |
| ---------------------------------------------------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Windows ReFS                                                     | CoW clone into the worktree's own `backend/target/`            | Source and destination on the same ReFS volume; verify that the OS and copy operation support block cloning                        |
| Windows NTFS                                                     | Independent target, built as needed                            | No native ReFS-style block cloning; do not substitute hardlinks or provision another volume                                        |
| macOS APFS                                                       | CoW clone into the worktree's own target                       | Source and destination on the same APFS volume, not merely in the same APFS container                                              |
| Linux Btrfs                                                      | File reflinks into the worktree's own target                   | Existing filesystem supports reflinks; no new subvolume or snapshot setup                                                          |
| Linux XFS                                                        | File reflinks into the worktree's own target                   | Existing filesystem has reflink enabled; do not reformat an older volume to enable it                                              |
| Linux ZFS                                                        | File block clones when supported; otherwise independent target | Existing pool features, OS and copy tool must support the actual operation; do not create datasets or snapshots for worktree setup |
| Linux ext4, macOS HFS+, or unsupported/cross-volume destinations | Independent target, built as needed                            | No verified native CoW clone for the selected paths                                                                                |

Use a small disposable file under the selected existing parents to verify clone support when it is not already established. A fast copy or a successful command alone does not prove shared blocks. A clone-only API/operation must succeed, or filesystem extent information must confirm sharing; modifying the destination must leave the source unchanged. Do not install system tools or change system settings just to run the probe. If cloning cannot be established, report the limitation and build an independent target instead of silently copying a large target tree.

Native operations include [ReFS block cloning](https://learn.microsoft.com/en-us/windows-server/storage/refs/block-cloning), [APFS cloning](https://developer.apple.com/documentation/foundation/about-apple-file-system), and [Btrfs reflinks](https://btrfs.readthedocs.io/en/stable/Reflink.html). On Linux, GNU `cp -a --reflink=always <source-target> <new-target>` requests reflinks and preserves attributes and links within the destination; the destination must not already exist. It fails when cloning is unsupported. Windows [Robocopy](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/robocopy/) and macOS [cp -c](https://github.com/apple-oss-distributions/file_cmds/blob/main/cp/cp.1) can fall back to ordinary copying, so their successful exit status alone is insufficient to establish CoW reuse. Do not use a full-copy fallback as the default for target seeding.

### Reuse policy

| Path (repo-relative)       | Approx size             | Policy                                                                  | Reason                                                                                                                                         |
| -------------------------- | ----------------------- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/sidecar/`   | ~213M                   | **Symlink → main**                                                      | gitignored downloaded cores (mihomo / clash-rs / clash / nyanpasu-service); branch-independent; re-fetch via `deno task prepare:check` is slow |
| `backend/tauri/resources/` | ~21M                    | **Symlink → main**                                                      | gitignored static assets (`geoip.dat`, `geosite.dat`, `Country.mmdb`, `wintun.dll`, service exes); branch-independent                          |
| `node_modules/`            | ~1.5G                   | **Independent `pnpm install`**                                          | pnpm global store already hardlink-dedupes; sharing risks concurrent lock conflicts                                                            |
| `backend/target/`          | tens to hundreds of GiB | **Independent CoW clone when supported; otherwise build independently** | Clones share unchanged data blocks while keeping files and Cargo locks independent; never share a writable target directory                    |
| `backend/tauri/tmp/dist/`  | build output            | **Independent — never symlink**                                         | branch-dependent frontend build; `emptyOutDir: true` means one worktree's `web:build` wipes the shared dir                                     |

Only `sidecar/` and `resources/` are symlink candidates.

### Reuse Cargo target through CoW

- Seed a new, absent `backend/target/` from a suitable existing target on the same clone-capable volume. Do not overwrite an existing worktree's target. A clone creates independent files; it must not hardlink destination files back to the source, use a symlink/junction to the source target, or point multiple worktrees at one `CARGO_TARGET_DIR`. Hardlinks between files entirely inside the new target may be preserved.
- Clone only while the source target is not being written by Cargo, rustc, rust-analyzer, or other build tools. A recursive file clone is not an atomic directory snapshot. Do not stop the developer's running processes to obtain a seed; if the source cannot be kept idle, skip seeding and build independently.
- Prefer a source built with the same resolved Rust toolchain version, target triple, lockfile, profile, features, and rustflags. Reuse an already suitable target; do not clean or rebuild the main checkout just to prepare a seed. Preserve timestamps, permissions and link semantics, and ensure copied links do not redirect writes to the source target.
- Treat the clone as a build cache, not a completed build for the new worktree. Cargo must validate it through the relevant build/check/test command before use. Absolute paths, build-script outputs and source timestamps can cause rebuilding even at the same commit. Do not rewrite Cargo fingerprints or cached paths to force reuse.
- Cloning saves initial data duplication, not every future build's space: rewritten or newly generated artifacts need storage. Directory size sums can double-count hardlinks and CoW data; assess savings through filesystem allocation information or measured volume-space changes.
- Keep `node_modules/` installation and `backend/tauri/tmp/dist/` generation independent. Do not extend target cloning into a shared frontend output directory.

For changes to worktree preparation tooling, verify clone support and write isolation with disposable files, then verify Cargo behavior after seeding: build/check/test as appropriate, rebuild after a source change, build two worktrees concurrently, and remove one without modifying the seed or the other worktree. Test the unsupported-filesystem path as well: it must leave a usable independent worktree without provisioning storage or performing an unannounced full target copy.

### Gitignored build prerequisites a fresh worktree lacks

- **`backend/tauri/tmp/dist`** — `backend/tauri/build.rs` calls `tauri_build::build()`, which validates `frontendDist: ./tmp/dist` **at compile time**. When missing, every `cargo build` / `clippy` / `cargo test --all-features` / rust-analyzer run on the tauri crate fails. Resolve one of:
  - Rust-only worktree → drop a placeholder (cheapest, no vite build).
  - Runnable UI → `pnpm web:build` (workspace packages resolve from source; this clears and refills `tmp/dist`).

`backend/tauri/tmp/git-info.json` is optional (`build.rs` guards it with `exists()`); run `deno task generate:git-info` only if accurate commit metadata must be baked in.

### Create a worktree

Commands shown for Windows / PowerShell (dir symlinks need Developer Mode, no elevation). Use symlinks only if the developer's existing settings permit them; do not enable Developer Mode or elevate to change the system for worktree setup. Choose `<worktree-path>` according to the filesystem policy above and `<type>/<name>` according to the task's branch convention. After creating the worktree, seed its target according to the CoW policy when supported, before launching build tools; otherwise leave it to Cargo to create independently.

```powershell
$main = git rev-parse --show-toplevel                 # capture main checkout root
git worktree add <worktree-path> -b <type>/<name>
cd <worktree-path>

# Reuse downloads only when existing settings allow directory symlinks
New-Item -ItemType SymbolicLink backend/tauri/sidecar   -Target "$main/backend/tauri/sidecar"
New-Item -ItemType SymbolicLink backend/tauri/resources -Target "$main/backend/tauri/resources"
# Otherwise prepare independent downloads: deno task prepare:check

# Seed backend/target through verified CoW cloning when supported.
# Otherwise let Cargo create it; never link it back to the main target.

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

Removal reclaims only the worktree's own files and its symlinks (pointers back to main) — it never touches the main checkout's real `sidecar/` / `resources/` or another target. For CoW target clones, shared data blocks remain allocated while another file still references them; deleting a clone releases only allocations no longer referenced.

## Git commits

### Stage only related files

Before committing, run `git status` to review the changes, stage only the files related to this change with explicit paths (`git add <specific-path>`), then verify with `git diff --cached --stat`.

Never use blanket staging such as `git add .`, `-A`, `--all`, or `*`. If something was staged by mistake, unstage it with `git reset HEAD <path>`.

### One commit does one thing

Every commit must be atomic, complete, and buildable.

When the user requests phased delivery, commit each completed, verified phase as
work progresses. Split phases by complete behavior, not by arbitrary file batches;
each commit must build without depending on later phases. Fold corrections into
the unpushed commit they complete, following the rules below.

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
