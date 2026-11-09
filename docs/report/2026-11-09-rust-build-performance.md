# Rust build and test performance investigation

Date: 2026-11-09

## Summary

The slow path is dominated by app-crate linking and filesystem writes, not by
rustc's CPU work. Full DWARF makes the app test binaries larger than 1 GiB, and
the app library builds `staticlib`, `cdylib`, and `rlib` outputs together. A
single source edit that relinks the app took 13m32s while using only 74s of user
CPU time and 230s of system time.

Two configuration changes remove most of the local feedback-loop cost:

1. Emit `line-tables-only` DWARF for the `dev` and `test` profiles.
2. Use Cargo content fingerprints instead of mtimes when deciding whether a
   build is fresh.

The first change reduces a full app test build from 15m32s to 6m22s and a
single-edit relink from 13m32s to 14.64s. The second change lets a new worktree
reuse a seeded target without rebuilding local crates solely because the new
checkout gave the sources newer timestamps.

## Environment

| Item              | Value                                                                |
| ----------------- | -------------------------------------------------------------------- |
| CPU               | 16 logical cores                                                     |
| Memory            | 31 GiB                                                               |
| Filesystem        | Btrfs with `compress=zstd:3`                                         |
| Rust (benchmark)  | `rustc 1.102.0-nightly (f5cedb83a 2026-11-07)`, `nightly-2026-11-08` |
| Rust (validation) | `rustc 1.102.0-nightly (5b7b37aef 2026-11-08)`, `nightly-2026-11-09` |
| Cargo             | `cargo 1.102.0-nightly (c07ac2c4d 2026-11-06)`                       |
| Linker            | rust-lld through the system `cc` driver                              |

The timings below are from repeated local worktree experiments on this machine.
They are intended to identify the dominant cost and compare configurations, not
to serve as a cross-machine benchmark suite.

## Root cause

### Full DWARF dominates app relinks

The app test binaries contain full DWARF for the dependency graph:

| Profile            | App library test | App binary test |
| ------------------ | ---------------: | --------------: |
| Full DWARF         |         1.43 GiB |        1.28 GiB |
| `line-tables-only` |          505 MiB |         357 MiB |

The full-DWARF single-edit relink reached a peak RSS of 6.57 GiB, with
`user=74s` and `sys=230s`. Reducing debug info reduced the same edit to
14.64s, with peak RSS around 3.0 GiB.

### Multiple crate types write extra large artifacts

`backend/tauri/Cargo.toml` declares:

```toml
crate-type = ['staticlib', 'cdylib', 'rlib']
```

The observation checkout contained a 3.88 GiB `libclash_nyanpasu_lib.a` and a
1.25 GiB `libclash_nyanpasu_lib.so` for a single app build. These outputs are
required by Tauri's mobile targets, but desktop development and tests do not
consume them.

### Target directories retain many historical hashes

The main checkout had a 67 GiB `backend/target`, with 47 GiB under
`debug/build` and 19 GiB under `debug/incremental`. The
`debug/build/clash-nyanpasu` directory alone was 31 GiB and contained 37
historical output directories. Changing profiles, linker flags, or rustflags
retains separate app outputs, each of which may include multi-gigabyte test
binaries and library artifacts.

### Worktree mtimes invalidate otherwise reusable targets

A fresh checkout gives all source files new mtimes. With Cargo's default
mtime-based freshness checks, seeding a target into a new worktree still
rebuilt roughly 40 local crates, taking 84.76s. Cargo content fingerprints
reduce the equivalent validation to about 0.5s when the target contains the
same source contents.

## Changes

### Line tables for development and tests

`backend/Cargo.toml` now sets:

```toml
[profile.dev]
debug = 'line-tables-only'

[profile.test]
debug = 'line-tables-only'
```

This keeps filenames and line numbers for backtraces while omitting variable,
type, and parameter details. Use `CARGO_PROFILE_DEV_DEBUG=2` or
`CARGO_PROFILE_TEST_DEBUG=2` for a temporary full-debug run when a debugger
needs local-variable information. The `release` and `profiling` profiles are
unchanged.

### Content fingerprints

The root `.cargo/config.toml` now enables Cargo's content-fingerprint mode:

```toml
[build]
fingerprint = 'content'

[unstable]
checksum-freshness = true
```

This is a nightly Cargo option. The repository already pins its Rust toolchain,
so builds use a consistent implementation. Cargo still uses mtimes for inputs
registered by build scripts with `cargo::rerun-if-changed`; the change is not a
complete replacement for timestamp handling.

## Results

### Build and link timing

| Scenario                        | Full DWARF | `line-tables-only` |
| ------------------------------- | ---------: | -----------------: |
| Full app test build             |     15m32s |              6m22s |
| Single app edit and test relink |     13m32s |             14.64s |
| App library test size           |   1.43 GiB |            505 MiB |
| App binary test size            |   1.28 GiB |            357 MiB |
| Peak RSS during edit            |   6.57 GiB |           2.99 GiB |

An additional seeded run with the same line-table profile completed the full
app test build in 4m37s. The same build on the final PR base produced a 500 MiB
library test binary and a 352 MiB binary test. A `cargo test --lib` edit
completed in about 11s.

### Content fingerprint behavior

| Operation                         |    Result |
| --------------------------------- | --------: |
| Initial fingerprint migration     |    82.15s |
| Cached `cargo check`              |     0.51s |
| Touch without content change      |     0.48s |
| Real source edit                  |     2.45s |
| Seeded target in a new worktree   |     0.52s |
| Switch back to mtime fingerprints | about 80s |

The initial migration is a one-time cost for an existing target. Do not toggle
between mtime and content modes for the same target. On the final PR base, the
migration took 99.2s and the following cached check took 0.50s.

### Test validation

`cargo test -p nyanpasu-core --all-features --lib` ran 1024 tests successfully
on the final PR base with the line-table profile. The tests themselves took
7.41s; compilation and linking took the remainder of the run.

`cargo test -p clash-nyanpasu --all-features --lib` ran 207 tests successfully
in 1.74s after the full app test build.

## Worktree target reuse

On this Btrfs filesystem, `cp -a --reflink=always` cloned a 97 GiB target in
13-18 seconds. `filefrag` confirmed shared extents and separate inodes.
Modifying the clone did not modify the seed, and deleting one clone did not
affect the source target.

A new worktree still needs its own writable target and independent
`backend/tauri/tmp/dist`. Only `backend/tauri/sidecar/` and
`backend/tauri/resources/` are appropriate symlinks. Submodules must be
initialized recursively:

```sh
git submodule update --init backend/nyanpasu-runtime backend/enable-loopback
git -C backend/nyanpasu-runtime submodule update --init --recursive
```

Two worktrees building concurrently did not contend on Cargo locks. Both took
about 127s versus about 80s for each build in isolation, which is expected CPU
and memory-bandwidth contention. Reflinks avoid the initial data copy, but new
or rewritten artifacts still consume new blocks.

## Rejected alternatives

| Option                                | Observation                                                                                     |
| ------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `-Z debuginfo-compression=zstd`       | Crashed rust-lld/LLVM 23.1 during linking                                                       |
| Explicit `lld` selection              | No improvement; the toolchain already uses rust-lld                                             |
| Gold linker                           | Slower full build at 16m15s                                                                     |
| `debug = 0` for the app only          | Worse edit time because dependencies still carried full DWARF                                   |
| `debug = 0` for dependencies          | Smaller test binaries, but no meaningful edit-time improvement                                  |
| `codegen-units = 16` with line tables | Slower line-table edit relink                                                                   |
| Split DWARF                           | Smaller incremental link in isolation, but adds `.dwo` files and debugging/packaging complexity |
| `-Zthreads` / `--jobs-frontend`       | About 27% faster app codegen, but higher RSS and no effect on the link bottleneck               |

## Follow-up

The largest remaining structural opportunity is separating the `staticlib` and
`cdylib` outputs required by Tauri mobile into a wrapper crate so desktop
development and tests only produce an `rlib`. That change requires a mobile
packaging design and should not be made by deleting the crate types directly.

For day-to-day work, `cargo check` and `cargo test --lib` avoid the full app
link. Full `cargo test --all-features` remains the pre-push and CI verification.
