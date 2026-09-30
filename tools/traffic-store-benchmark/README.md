# Traffic store evaluation

This independent workspace keeps the experimental Turso SDK out of the app's
dependency graph. Its tests reference the app's actor and client files directly;
there is no second actor implementation. Turso is enabled by default here only.
The executable is an isolated test harness sharing the Tauri actor source, not
the complete application. Its RSS measurements do not describe full app RSS.
Its release profile matches the app: size optimization, LTO, one codegen unit,
and unwinding panics.

Build the benchmark test executable:

```powershell
cargo test --manifest-path tools/traffic-store-benchmark/Cargo.toml --locked --release --no-run
```

Pass the emitted test executable path to
`scripts/traffic/run_store_benchmark.ps1`. The runner starts a fresh process for
each store and workload. Existing evaluation results retain their original
source hashes; relocating this harness does not regenerate those measurements.

For a small correctness smoke run, set `NYANPASU_TRAFFIC_PERF_CASE=1000-active`
and `NYANPASU_TRAFFIC_PERF_STORE=redb` (then `turso`) and run:

```powershell
cargo test --manifest-path tools/traffic-store-benchmark/Cargo.toml --locked -- --ignored --exact benchmark::measured_session_workloads
```

Debug smoke timings validate execution only and are not comparison results.
On Windows, the Turso SDK build requires `rc.exe` on `PATH`; see the
[adapter prerequisites](../../backend/nyanpasu-traffic-turso/README.md).
