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

## Worktrees and resource reuse

Feature/migration work runs in isolated git worktrees by default. Working in the current checkout is an option when the user chooses it after a cost assessment. Before implementation:

- Consider the task's scope, expected duration, concurrent work, and existing uncommitted changes in the current checkout.
- Weigh the isolation benefits against dependency installation, independent Cargo builds, disk usage, and preparation of gitignored build prerequisites. Reuse a suitable existing worktree when available.
- Summarize the relevant costs and benefits, state a recommendation, then ask the user whether to work in a worktree or the current checkout. Wait for their choice before implementation; if they have already specified a choice for the task, follow it without asking again.
- Keep isolated worktrees as the recommended default for feature/migration work. Small, isolated edits may be cheaper in the current checkout; broad changes or concurrent work strengthen the case for a worktree. The assessment adds a user-selectable alternative, not a replacement for the isolation and resource reuse policy.

When the user chooses a worktree, its location is the developer's choice (any path outside the repo tree). Worktrees share the main `.git`. The rule: reuse expensive **branch-independent** assets from the main checkout via symlink, and regenerate everything **branch-dependent** per worktree.

### Reuse policy

| Path (repo-relative)       | Approx size  | Policy                          | Reason                                                                                                                                    |
| -------------------------- | ------------ | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/sidecar/`   | ~213M        | **Symlink → main**              | gitignored downloaded cores (mihomo / clash-rs / clash / nyanpasu-service); branch-independent; re-fetch via `pnpm prepare:check` is slow |
| `backend/tauri/resources/` | ~21M         | **Symlink → main**              | gitignored static assets (`geoip.dat`, `geosite.dat`, `Country.mmdb`, `wintun.dll`, service exes); branch-independent                     |
| `node_modules/`            | ~1.5G        | **Independent `pnpm install`**  | pnpm global store already hardlink-dedupes; sharing risks concurrent lock conflicts                                                       |
| `backend/target/`          | ~50G         | **Independent — never symlink** | sharing causes Cargo incremental-fingerprint churn + concurrent build-lock waits across diverged source trees                             |
| `backend/tauri/tmp/dist/`  | build output | **Independent — never symlink** | branch-dependent frontend build; `emptyOutDir: true` means one worktree's `web:build` wipes the shared dir                                |

Only `sidecar/` and `resources/` are symlink candidates.

### Gitignored build prerequisites a fresh worktree lacks

- **`frontend/interface/dist`** — `@nyanpasu/interface` (`main` → `./dist/index.js`) is consumed by `@nyanpasu/nyanpasu`. Produce with `pnpm -F interface build`.
- **`backend/tauri/tmp/dist`** — `backend/tauri/build.rs` calls `tauri_build::build()`, which validates `frontendDist: ./tmp/dist` **at compile time**. When missing, every `cargo build` / `clippy` / `cargo test --all-features` / rust-analyzer run on the tauri crate fails. Resolve one of:
  - Rust-only worktree → drop a placeholder (cheapest, no vite build).
  - Runnable UI → `pnpm web:build` (build `interface` first; it clears and refills `tmp/dist`).

`backend/tauri/tmp/git-info.json` is optional (`build.rs` guards it with `exists()`); run `pnpm generate:git-info` only if accurate commit metadata must be baked in.

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
pnpm -F interface build                               # -> frontend/interface/dist (gitignored)

# Satisfy tauri-build's frontendDist check — pick one:
New-Item -ItemType Directory -Force backend/tauri/tmp/dist | Out-Null            # A) Rust-only placeholder
Set-Content backend/tauri/tmp/dist/index.html '<!doctype html><title>dev</title>'
# pnpm web:build                                      # B) real UI (replaces tmp/dist)
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
