# Testing and review

## Design tests around boundaries

- Prefer testing pure services directly with plain values.
- For infrastructure dependencies, define narrow traits and inject them.
- Traits that are intended to be mocked should be compatible with `mockall` / `automock` where practical.
- Keep mock-only APIs behind `#[cfg(test)]` or test-support modules.
- Do not use global test fixtures for application services. Construct a test `NyanpasuClient` or test-specific service graph.
- Actor tests should spawn the actor with fake adapters and send typed messages through its typed client.
- Avoid sleeping in actor tests. Prefer explicit acknowledgements, request/reply messages, or test hooks.

Example mockable trait:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait UiEventSink: Send + Sync + 'static {
    fn emit_state_changed(&self, event: StateChanged) -> anyhow::Result<()>;
}
```

Another acceptable trait shape:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait ConfigStore: Send + Sync + 'static {
    fn load(&self) -> anyhow::Result<Vec<u8>>;
    fn save(&self, bytes: &[u8]) -> anyhow::Result<()>;
}
```

## Run the relevant checks

Use named Deno tasks for repository tooling; the root `package.json` delegates
its existing script commands to these tasks:

| Check                       | Command                              |
| --------------------------- | ------------------------------------ |
| Frontend tests              | `pnpm test:frontend`                 |
| Backend tests               | `pnpm test:backend`                  |
| Repository script tests     | `deno task test:scripts`             |
| Architecture ledger tests   | `deno task test:architecture-ledger` |
| Architecture migration gate | `deno task lint:architecture-ledger` |
| Frontend types              | `pnpm typecheck`                     |
| Project lint                | `pnpm lint`                          |

Choose tests that exercise the changed behavior and complete required checks. Rust
checks need the build prerequisites described in [workflow](workflow.md). See the
[frontend test guide](../frontend-testing.md) for Vitest setup and test placement,
and [Windows bundle testing](../testing/windows-bundle.md) for packaged validation.
Tests must use temporary or injected paths rather than real user configuration.

## Review before submitting

- Each changed line supports the request; assumptions and success criteria are clear.
- Services have an explicit role and injected dependencies, without new mutable global state.
- Actors keep ownership private, use their mailbox for serialization, and avoid RPC cycles.
- Tauri and infrastructure remain behind adapters; `NyanpasuClient` stays a facade.
- No new Tauri dependency enters `NyanpasuClient`, typed clients, actors, or pure services
  ([core and frontend separation](architecture.md#core-and-frontend-separation)).
- Compatibility layers explain their blocker and removal condition.
- Shared RPC changes preserve capability restrictions, owner isolation, errors, and events on both transports.
- Tests use pure values, injection, or boundary fakes and verify meaningful behavior.
- Relevant checks pass, generated bindings are current, and remaining limitations are documented.
- Language style follows the [Rust](rust.md) or [TypeScript and React](typescript.md) guide. Review React slots, shared constants, logical grouping, and project UI reuse as well as lint results.
- The commit is atomic and only related files are staged.
