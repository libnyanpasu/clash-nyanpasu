# Rust code style

This guide records the existing Rust style and lint setup. It does not introduce
a new lint profile. Follow [architecture and ownership](architecture.md) for
service boundaries and [unified RPC](rpc.md) for application commands.

## Formatting and imports

The root `rust-toolchain.toml` selects nightly. The application workspace uses
Rust 2024 and `backend/rustfmt.toml`:

| Setting             | Current convention                                                         |
| ------------------- | -------------------------------------------------------------------------- |
| Line width          | 100 columns                                                                |
| Indentation         | Four spaces, no hard tabs                                                  |
| Newlines            | `Auto`, as configured by Rustfmt                                           |
| Imports and modules | Reordered; imports grouped at crate granularity                            |
| Derives             | Merged by the formatter                                                    |
| Initializers        | Explicit field names (`use_field_init_shorthand = false`)                  |
| Other syntax        | Remove nested parentheses; preserve configured heuristics and explicit ABI |

Let Rustfmt format imports, multiline expressions, attributes, and where clauses.
Use `snake_case` for modules/functions/fields, `PascalCase` for types and variants,
and `SCREAMING_SNAKE_CASE` for constants. Match the surrounding module's API and
visibility conventions; avoid unrelated formatting or module reorganizations.

## Types, errors, and ownership

- Use the role-based names in the architecture guide (`StateActor`, `StateClient`,
  `RuntimeBuilder`, adapter names). Keep dependencies visible in fields and constructors.
- Prefer explicit domain structs/enums and typed messages over string dispatch.
- Propagate recoverable errors with `Result` and `?`; follow the consuming crate's
  existing error type rather than adding a workspace-wide error convention.
- Keep public traits and methods narrow. Preserve ownership and borrowing instead
  of introducing shared locks or clones solely to bypass a design problem.
- A panic represents an invariant violation. Do not catch it in production or map
  a panicking task's `JoinError` into an ordinary recoverable error.
- Use `#[cfg(...)]` for platform/test boundaries and keep test-only APIs out of
  production interfaces. Actor tests use injected adapters and explicit replies.
- Comments explain invariants, lifecycle assumptions, and migration blockers.
  Legacy bridges need the documented `TODO(actor-migration)`/`FIXME(actor-migration)`
  reason and removal condition.

## Checks and suppressions

```sh
pnpm lint:rustfmt
pnpm lint:clippy
# To apply formatting:
cargo fmt --manifest-path ./backend/Cargo.toml --all
```

The Clippy command checks all targets and all features. Keep its current severity
and existing scoped allowances; this guide adds no `pedantic` policy or blanket
`-D warnings`. Fix issues in touched code. If an allowance is needed, keep it at
the narrowest relevant scope and explain the specific reason.

Rust files are excluded from Prettier. Backend staged changes already run Clippy
and Rustfmt through `.lintstagedrc.js`. Tests are separate; run those relevant to
the behavior as described in [testing and review](testing.md). Tauri checks also
need the [gitignored build prerequisites](workflow.md#gitignored-build-prerequisites-a-fresh-worktree-lacks).
The `backend/nyanpasu-runtime` submodule owns its own workspace and formatting
configuration; do not apply the parent workspace's conventions to it implicitly.
