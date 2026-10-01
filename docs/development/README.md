# Development standards

These are the current development rules for human contributors and coding agents.
Read them before changing application behavior. They organize the shared rules from
[`AGENTS.md`](../../AGENTS.md) by the task you are doing; the agent instructions
retain their operational checklist and section numbers.

## Choose a guide

| When you are working on                                              | Read                                             |
| -------------------------------------------------------------------- | ------------------------------------------------ |
| Services, state ownership, dependency injection, or legacy migration | [Architecture and ownership](architecture.md)    |
| Commands, frontend backend calls, HTTP capabilities, or events       | [Unified RPC](rpc.md)                            |
| Tests, mocks, verification, or final review                          | [Testing and review](testing.md)                 |
| Worktree selection, build prerequisites, or commits                  | [Development workflow](workflow.md)              |
| Rust style and current formatter/lint behavior                       | [Rust code style](rust.md)                       |
| TypeScript, React structure, constants, slots, or UI composition     | [TypeScript and React code style](typescript.md) |

Start with the [root README](../../README.md#development) for running the app and
[CONTRIBUTING.md](../../CONTRIBUTING.md) for contribution setup. For frontend test
setup, see the [Vitest guide](../frontend-testing.md).

## Rules to keep in mind

- Construct dependencies explicitly and keep long-lived mutable state with its actor owner.
- Call application operations through `NyanpasuClient`; keep infrastructure behind adapters.
- Use the unified RPC surface for application APIs on both desktop and HTTP transports.
- Wait for real in-process RPC results; a caller leaving does not cancel owner-started work.
- Keep changes focused, verify the intended behavior, and submit atomic commits.
- Prefer isolated worktrees for feature/migration work; review costs before choosing the current checkout.

## Maintaining these rules

Update the relevant guide and `AGENTS.md` together when changing a shared rule.
`CLAUDE.md` imports `AGENTS.md`, so the same instructions apply there. Neither human
nor agent documentation should introduce a separate architectural exception.

The documents under `docs/plan/`, `docs/superpowers/`, and `docs/reviews/` record
historical proposals and decisions. Use these guides and `AGENTS.md` for current
rules; older plans may contain superseded timeout or lifecycle policies. Consult
the [actor migration roadmap](../design/actor-migration-roadmap.md) for migration
status and outstanding work, rather than treating every historical plan as policy.
