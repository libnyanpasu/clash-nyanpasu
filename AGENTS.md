# AGENTS.md

The application backend uses explicit dependency injection, actor-owned state, and pure domain services; the migration away from `::global()` singletons and Tauri-coupled services is complete. The current goal is to separate `NyanpasuClient` from the Tauri GUI into `nyanpasu-core` (see section 6).

## Mandatory Reading: Development Standards

Before starting any repository work, agents MUST read [docs/development/README.md](docs/development/README.md) and all standards guides: [architecture](docs/development/architecture.md), [unified RPC](docs/development/rpc.md), [testing and review](docs/development/testing.md), [workflow](docs/development/workflow.md), [Rust code style](docs/development/rust.md), [TypeScript and React code style](docs/development/typescript.md), [frontend packages](docs/development/frontend-packages.md), and [repository scripts](docs/development/scripts.md).

Agents MUST strictly follow these development standards together with the instructions below. Reading this file alone is insufficient. The guides are mandatory project requirements, not optional background or suggestions. If a guide is unavailable or the current requirements conflict, report the issue and resolve it before proceeding with affected work.

Behavioral guidelines reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 0. Synchronization Policy

`CLAUDE.md` imports this file, so both tools follow the same instructions. The guides under `docs/development/` are the single source of the detailed rules; this file routes to them and restates only the rules that are violated most often.

- When a shared rule changes, update its guide in the same change, and update this file only if it restates that rule.
- Do not copy guide sections into this file; link to them.
- Do not create a Claude-only or agent-only exception unless the tool truly requires it.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:

- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

For small, obvious tasks, do this briefly. For architecture, migration, or cross-module work, be explicit.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

This rule does not override the architecture rules. Do not use `::global()` or hidden mutable process state merely because it is fewer lines.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:

- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:

- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

For core/GUI separation work, the allowed scope is the smallest call path needed to move the touched service or API without leaving a hidden compatibility layer behind.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:

- "Add validation" -> "Write tests for invalid inputs, then make them pass"
- "Fix the bug" -> "Write a test that reproduces it, then make it pass"
- "Refactor X" -> "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```text
1. [Step] -> verify: [check]
2. [Step] -> verify: [check]
3. [Step] -> verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## 5. Architecture

The actor/dependency-injection migration is complete: application services are explicitly constructed, mutable state has a serial owner, and Tauri sits behind adapters. The service-design rules — target architecture, ractor usage, actor vs. pure service vs. adapter classification, state commits, compatibility policy, and role names — live in [Architecture and ownership](docs/development/architecture.md). Read it before adding or changing a service; do not re-derive these trade-offs from this file.

Non-negotiable rules, restated because they are the most common violations:

- Do not add global service singletons (`::global()`, `static` `OnceCell`/`OnceLock`/`Lazy` service state). The `deno task lint:architecture-ledger` gate enforces this.
- Dependencies are explicit: constructors, builders, function parameters, or actor startup arguments.
- `NyanpasuClient` is a facade with domain operations, never a service locator or a raw `ActorRef` registry.
- In-process request/reply waits for the real result; deadlines belong to network/IPC adapters. Dropping a caller does not cancel owner-started work.
- Business logic does not touch Tauri types; infrastructure goes behind narrow, consumer-owned port traits.
- Backend packages follow [Backend packages](docs/development/backend-packages.md): `nyanpasu-core` owns use cases, typed actor clients and consumed ports; `nyanpasu-platform` implements infrastructure ports and may depend on core; core must not depend on platform in production. Neither neutral crate nor `nyanpasu-config` may depend on Tauri or egui/eframe. Run `deno task lint:backend-boundaries` after backend package-boundary changes.

## 6. Current Goal: Separate `NyanpasuClient` from the GUI

`NyanpasuClient` and the application layer beneath it are moving out of the Tauri crate into `backend/nyanpasu-core`, so that a Tauri GUI and a future `nyanpasu-cli` are both frontends over the same core. This split is the prerequisite for mobile support. See [Core and frontend separation](docs/development/architecture.md#core-and-frontend-separation) for the target layout and rules.

When touching code under `backend/tauri/src/client/` or the actors, services, and ports it depends on:

- Do not add new `tauri` dependencies (`AppHandle`, `tauri::State`, `tauri::async_runtime`, Tauri events, windows, tray) to the client, actors, or pure services. Add a port trait and implement it in the GUI crate instead.
- Keep frontend-specific behavior (windows, tray, webview events, dialogs, main-thread execution, Tauri IPC) in the GUI crate as adapters.
- Move code into its neutral application/domain owner by updating callers, not by leaving re-export shims behind in the Tauri crate.

## 7. Unified RPC

Read [Unified RPC](docs/development/rpc.md) before adding or changing a command, an HTTP capability, a frontend backend call, or a shared event.

- Declare application commands with `#[nyanpasu_macro::rpc(...)]` and register them in `backend/tauri/src/specta_export.rs` (queries vs. mutations by effect, not by name). Regenerate bindings; never hand-edit them.
- Commands are thin adapters: parse the DTO, call `NyanpasuClient`, map errors. No business orchestration in commands.
- HTTP is explicit opt-in via `rpc(http)`. Never inject `AppHandle` into an HTTP-enabled command or `RpcDependencies`.
- The frontend calls the backend only through `@nyanpasu/rpc` and `@nyanpasu/query`; no raw `invoke` or ad hoc HTTP fetches.

## 8. Testing

Read [Testing and review](docs/development/testing.md) for test design and the checks to run. Test pure services with plain values and actors through their typed clients with fake adapters; do not use global fixtures or sleeps.

## 9. Code Style and Repository Conventions

| Area                                                                     | Guide                                                                 |
| ------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| Rust formatting, lints, types, and errors                                | [Rust code style](docs/development/rust.md)                           |
| TS/TSX, React grouping, constants, `data-slot`, UI components, i18n keys | [TypeScript and React code style](docs/development/typescript.md)     |
| Frontend package ownership and dependency direction                      | [Frontend packages](docs/development/frontend-packages.md)            |
| Repository scripts and Deno tasks                                        | [Repository scripts](docs/development/scripts.md)                     |
| GitHub workflow display names                                            | [Development workflow](docs/development/workflow.md#github-workflows) |

Formatting and lint passing does not finish the review: semantic naming, reuse, logical grouping, and visual consistency are mandatory review requirements in those guides.

## 10. Worktrees

Follow [Worktrees and resource reuse](docs/development/workflow.md#worktrees-and-resource-reuse).

- Before implementing feature/refactoring work, assess the cost, recommend a worktree or the current checkout, and ask the user unless they have already chosen.
- In a worktree, only `backend/tauri/sidecar/` and `backend/tauri/resources/` may be symlinked to the main checkout. Never share `backend/target/` or `backend/tauri/tmp/dist/`.

## 11. Git Commits

Follow [Git commits](docs/development/workflow.md#git-commits).

- Stage explicit paths only; never `git add .`, `-A`, `--all`, or `*`.
- One commit does one thing and builds. No fix-up commits: fold a correction into the unpushed commit it completes. Rewriting pushed history requires explicit consent.
- The subject is imperative, at most 72 characters, without a trailing period; a non-trivial change explains why in the body.

## 12. Final Review

Before finishing, walk through [Review before submitting](docs/development/testing.md#review-before-submitting). In particular: every changed line traces to the request, success criteria were verified, and no global service state or new Tauri dependency entered the client, actors, or pure services.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, clarifying questions come before implementation rather than after mistakes, and new code stays GUI-independent and explicitly composed.
