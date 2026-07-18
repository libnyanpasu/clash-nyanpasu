# fake-core

Test-only deterministic fake Clash core binary and parent-side protocol helpers.

This package exists so process-lifecycle tests can inject check / start / apply failures without real cores, OS proxy, sleep-based ordering, or production sidecars. It is **never** packaged as a production sidecar or resource (`publish = false`; workspace member + `app`/`tauri` **dev-dependency** only).

## Overview

`fake-core` provides:

1. A std-only binary that accepts the **real core argv shapes** used by `nyanpasu-utils`.
2. Behaviour controlled exclusively by **`FAKE_CORE_*` environment variables**.
3. A TCP **READY / RELEASE** control barrier for deterministic parent/child ordering.
4. Optional loopback port hold + status-injection-only HTTP apply endpoint.
5. A library of parent helpers: binary discovery, barrier, RAII child reaping, command builders.

Production packaging, real Clash API surface, TUN/service modes, and OS proxy are explicitly out of scope.

## Features

- **Real argv compatibility**: check and start shapes match mihomo / clash-rs / premium invocation.
- **Strict env failure**: unset may default; set-but-invalid numeric/UTF-8 values fail with exit `2` and a stable `invalid configuration` message before long-running work.
- **Barrier ordering**: long-running start requires `FAKE_CORE_READY_ADDR`; no barrier and no `START_EXIT` → fail-fast exit `2` (no infinite park, no sleep ordering).
- **Dynamic ports**: `HOLD_PORT` / `HTTP_PORT` of `0` bind ephemeral ports; the READY frame announces the actual bound values.
- **Exact apply injection**: only exact `PUT /configs` and `PATCH /configs` receive injected status/body; prefix paths such as `/configs/xxx` return `404`.
- **ScopedChild RAII**: drop kills + waits still-running children so assertion failures cannot leak processes or held ports.
- **Cross-crate discovery**: `NYANPASU_FAKE_CORE` → current_exe profile sibling → `$CARGO_TARGET_DIR/{debug|release}/fake-core`.

## Argv shapes

| Mode             | Shape                                   |
| ---------------- | --------------------------------------- |
| check            | `fake-core -t -d <app_dir> -f <config>` |
| start (mihomo)   | `fake-core -m -d <app_dir> -f <config>` |
| start (clash-rs) | `fake-core -d <app_dir> -c <config>`    |
| start (premium)  | `fake-core -d <app_dir> -f <config>`    |

Unknown flags, missing `-d`, or missing `-f`/`-c` → stderr + exit `2`.

## Environment contract

### Binary selection (parent harness)

| Key                  | Meaning                                                                                    |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `NYANPASU_FAKE_CORE` | Optional absolute path override. Empty values are **ignored** (fall through to discovery). |

Binary selection is intentionally separate from behaviour keys.

### Behaviour keys (`FAKE_CORE_*`)

| Key                            | Mode  | Meaning                                                                   | Default when unset |
| ------------------------------ | ----- | ------------------------------------------------------------------------- | ------------------ |
| `FAKE_CORE_CHECK_EXIT`         | check | Exit code                                                                 | `0`                |
| `FAKE_CORE_CHECK_STDOUT`       | check | Raw stdout bytes (lossy if non-UTF-8 OS string)                           | none               |
| `FAKE_CORE_CHECK_STDERR`       | check | Raw stderr                                                                | none               |
| `FAKE_CORE_START_EXIT`         | start | **Immediate** termination after optional start streams. Not long-running. | unset              |
| `FAKE_CORE_START_STDOUT`       | start | Optional stdout before exit / barrier                                     | none               |
| `FAKE_CORE_START_STDERR`       | start | Optional stderr before exit / barrier                                     | none               |
| `FAKE_CORE_READY_ADDR`         | start | Parent control listener `host:port`. **Required** for long-running start  | unset              |
| `FAKE_CORE_HOLD_PORT`          | start | Port to hold on `127.0.0.1`. `0` = ephemeral                              | unset              |
| `FAKE_CORE_HTTP_PORT`          | start | HTTP status-injection port on `127.0.0.1`. `0` = ephemeral                | unset              |
| `FAKE_CORE_APPLY_STATUS`       | start | Status for exact `PUT`/`PATCH /configs`                                   | `204`              |
| `FAKE_CORE_APPLY_BODY`         | start | Response body for exact apply paths                                       | empty              |
| `FAKE_CORE_EXIT_AFTER_RELEASE` | start | Exit code after RELEASE (or parent EOF)                                   | `0`                |

**Strictness:** if a key is set, it must parse as the expected integer range (`u8` 0–255 or `u16` 0–65535) and must be valid UTF-8 for keys read as text. Invalid → exit `2` with `fake-core: invalid configuration: <key>=...`.

**Start modes:**

- `FAKE_CORE_START_EXIT=<code>` → print optional streams, exit immediately (ports / READY are not used).
- Long-running start → requires non-empty `FAKE_CORE_READY_ADDR`. Holds optional ports, announces actual ports in READY, waits for RELEASE, then exits with `EXIT_AFTER_RELEASE`.
- Neither `START_EXIT` nor barrier → exit `2` with a message naming both keys.

## TCP READY / RELEASE barrier

Parent and child never order steps with sleep. Timeouts are deadlock safety nets only.

1. Parent binds `ReadyBarrier::bind_local()` (`127.0.0.1:0`) and passes `addr_string()` as `FAKE_CORE_READY_ADDR`.
2. Child binds optional hold/http ports, then connects to the barrier and writes one READY line.
3. Parent `accept_ready(timeout)` returns `ReadyConnection { stream, announcement }`.
4. Parent later writes `RELEASE\n` via `ReadyBarrier::release(stream)` (or drops the socket; child treats UnexpectedEof as release).
5. Child exits with `FAKE_CORE_EXIT_AFTER_RELEASE` (default `0`).

READY line formats:

```text
READY
READY hold=18080
READY http=19090
READY hold=18080 http=19090
```

When configured hold and http ports are equal (including both `0`), the child binds **one** listener, serves HTTP on it, and announces the same actual port for both fields.

## HTTP apply contract

- Bind: `127.0.0.1` only.
- Matched request targets: exact `PUT /configs` and exact `PATCH /configs`.
- Response: injected `FAKE_CORE_APPLY_STATUS` + `FAKE_CORE_APPLY_BODY`.
- Everything else (including `/configs/extra`, other methods, other paths): `404 Not Found` with empty body.
- This is **status injection only** — no Clash config semantics, no full HTTP API.

## Binary discovery and prebuild

`CARGO_BIN_EXE_fake-core` is set **only** for this package's own integration tests. A `dev-dependency` on `fake-core` does **not** build the binary and does **not** set that env for the dependent package.

Discovery order for cross-crate consumers (`resolve_bin_path` / `require_bin_path`):

1. Non-empty `NYANPASU_FAKE_CORE`
2. Sibling of the running test binary's profile directory (`.../<profile>/deps` → `.../<profile>/fake-core[.exe]`)
3. `$CARGO_TARGET_DIR/{debug|release}/fake-core` (or `backend/target/...` relative to this crate when unset)

Prebuild for focused consumer tests:

```bash
cargo build -p fake-core
# or
cargo test -p fake-core
```

`require_bin_path()` errors include the stable `PREBUILD_COMMAND` (`cargo build -p fake-core`).

## Build and test usage

Same-package (binary guaranteed by Cargo):

```bash
cargo test -p fake-core
```

Same-package tests should prefer `env!("CARGO_BIN_EXE_fake-core")`.

Cross-crate consumer (e.g. tauri process matrix under `cfg(test)`):

```bash
cargo build -p fake-core
cargo test -p clash-nyanpasu --lib process_core_bridge -- --nocapture
```

Minimal parent-side long-running start (library helpers, no invented constructors):

```rust
use fake_core::{FakeCoreCommand, ReadyBarrier, env_keys};
use std::{path::Path, process::Stdio, time::Duration};

fn long_running_start(bin: &Path, app_dir: &Path, config: &Path) {
    let barrier = ReadyBarrier::bind_local().expect("bind barrier");
    let mut child = FakeCoreCommand::new(bin)
        .start_mihomo(app_dir, config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HTTP_PORT, "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn");

    let ready = barrier
        .accept_ready(Duration::from_secs(5))
        .expect("READY");
    let _http = ready.announcement.http_port;
    ReadyBarrier::release(ready.stream).expect("RELEASE");
    let status = child.wait_with_timeout(Duration::from_secs(5)).expect("wait");
    assert!(status.success());
}
```

Immediate check/start failure injection uses `FakeCoreCommand::check` / `start_*` plus `FAKE_CORE_CHECK_EXIT` / `FAKE_CORE_START_EXIT` and `output()`.

`PathEnvGuard` / `PathEnvLock` serialize concurrent tests that set or observe `NYANPASU_FAKE_CORE` (libtest may run multi-threaded; `set_var` is not race-free alone).

## Public surface (library)

| Item                                         | Role                                                                                                   |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `FakeCoreCommand`                            | Builder for check / mihomo / clash-rs / premium argv + env/stdio + `spawn` / `spawn_scoped` / `output` |
| `ScopedChild`                                | RAII kill+wait on drop; `wait_with_timeout`, `into_inner`                                              |
| `ReadyBarrier`                               | Parent control listener: `bind_local`, `accept_ready`, `release`                                       |
| `ReadyConnection`                            | Accepted control stream + `ReadyAnnouncement`                                                          |
| `ReadyAnnouncement`                          | Parsed/serialized READY fields (`hold_port`, `http_port`)                                              |
| `Mode` / `parse_args`                        | Argv → Check/Start                                                                                     |
| `env_keys::*`                                | Stable behaviour key constants                                                                         |
| `PATH_ENV` / `BIN_NAME` / `PREBUILD_COMMAND` | Discovery constants                                                                                    |
| `resolve_bin_path` / `require_bin_path`      | Cross-crate binary lookup                                                                              |
| `PathEnvLock` / `PathEnvGuard`               | Test-only serialization of `NYANPASU_FAKE_CORE` mutations                                              |
| `signal_ready_and_wait_release`              | Child-side barrier (used by the binary)                                                                |
| `wait_with_timeout`                          | Test utility (deadlock safety net; also on `ScopedChild`)                                              |

There is no production service API and no `Object::new()`-style facade.

## Directory structure

```text
backend/fake-core/
├── Cargo.toml          # publish = false; std-only deps
├── README.md
├── DESIGN.md
├── src/
│   ├── lib.rs          # protocol helpers + discovery + unit tests
│   └── main.rs         # fake-core binary
└── tests/
    └── protocol.rs     # barrier / env / HTTP / ScopedChild integration tests
```

## Cross-platform limitations

- **std-only** intentionally: no async runtime, no HTTP framework, no production core linkage.
- Bind address is **loopback only** (`127.0.0.1`).
- `ScopedChild` kill+wait is portable; the integration test that asserts `/proc/<pid>` disappearance is **Linux-only**. On other platforms, drop still kills+waits, but PID tombstone assertions are skipped.
- Windows service-mode, TUN privilege paths, and real sidecar packaging are **not** covered here; those remain S10 manual smoke / production code.
- Focused consumer package tests require an explicit prebuild (Cargo does not build this bin solely because of a path `dev-dependency`).

## Related documentation

- [DESIGN.md](DESIGN.md) — goals, lifecycle, threat model, S09/S10 ownership
- PR-4S: `docs/superpowers/specs/2026-07-13-pr4s-actor-migration-stabilization/`
- Consumer adapter: `backend/tauri/src/client/process_core_bridge.rs` (`cfg(test)` only)
