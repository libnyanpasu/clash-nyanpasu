# Development standards

These are the current development rules for human contributors and coding agents.
Read them before changing application behavior. They are the single source of the
detailed rules; [`AGENTS.md`](../../AGENTS.md) adds agent behavior guidelines, routes
to these guides, and restates only the most frequently violated rules.

## Choose a guide

| When you are working on                                             | Read                                             |
| ------------------------------------------------------------------- | ------------------------------------------------ |
| Services, state ownership, dependency injection, or core/GUI split  | [Architecture and ownership](architecture.md)    |
| Commands, frontend backend calls, HTTP capabilities, or events      | [Unified RPC](rpc.md)                            |
| Tests, mocks, verification, or final review                         | [Testing and review](testing.md)                 |
| Worktree selection, build prerequisites, or commits                 | [Development workflow](workflow.md)              |
| Repository scripts, Deno tasks, tool dependencies and script layout | [Repository scripts](scripts.md)                 |
| Rust style and current formatter/lint behavior                      | [Rust code style](rust.md)                       |
| TypeScript, React structure, constants, slots, or UI composition    | [TypeScript and React code style](typescript.md) |

Start with the [root README](../../README.md#development) for running the app and
[CONTRIBUTING.md](../../CONTRIBUTING.md) for contribution setup. For frontend test
setup, see the [Vitest guide](../frontend-testing.md).
For package ownership, dependency directions, and source-only typechecking, see
[Frontend packages](frontend-packages.md).

## Rules to keep in mind

- Construct dependencies explicitly and keep long-lived mutable state with its actor owner.
- Call application operations through `NyanpasuClient`; keep infrastructure behind adapters.
- Keep `NyanpasuClient`, actors, and pure services free of new Tauri dependencies; they are moving to `nyanpasu-core`.
- Use the unified RPC surface for application APIs on both desktop and HTTP transports.
- Wait for real in-process RPC results; a caller leaving does not cancel owner-started work.
- Keep changes focused, verify the intended behavior, and submit atomic commits.
- Keep repository scripts under `scripts/src/` and call public entrypoints through `deno task`.
- Prefer isolated worktrees for feature/refactoring work; review costs before choosing the current checkout.

## Maintaining these rules

Update the relevant guide when changing a shared rule, and update `AGENTS.md` in
the same change only if it restates that rule.
`CLAUDE.md` imports `AGENTS.md`, so the same instructions apply there. Neither human
nor agent documentation should introduce a separate architectural exception.

The documents under `docs/spec/`, `docs/plan/`, `docs/report/`, and `docs/review/` record
historical proposals and decisions. Use these guides and `AGENTS.md` for current
rules; older plans may contain superseded timeout or lifecycle policies. The actor
migration is complete; the [actor migration roadmap](../design/actor-migration-roadmap.md)
is a historical record of how it was done, not a list of current work.
