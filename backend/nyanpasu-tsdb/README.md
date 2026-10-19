# nyanpasu-tsdb

Time-series storage for statistics and logs, shared by the traffic statistics, core logs and,
later, job logs. It provides:

- a label catalog with a byte budget and a fold ladder;
- pre-aggregated series tiers, such as minute and hour usage;
- append-only event streams, such as closed connections and log records, with logical
  cursors;

all over a replaceable storage backend: in memory for tests, and redb as the first
persistent backend (feature `redb`).

The crate is a neutral library:

- It starts no thread, timer or pool, and holds no global state.
- The consumer's actor owns the store, and makes every write and maintenance call, each
  with a work budget.
- Queries run outside that actor, on snapshots.

Charged store allocations are bounded by an injected resource profile. Presets remain
provisional; the mobile design targets an extension sharing 50 MiB with its host, not a measured
process-RSS guarantee. Measurements and the platform verification matrix are in
[BENCHMARKS.md](BENCHMARKS.md#e13-profile-evidence-and-verification-matrix).

The design is [`docs/design/nyanpasu-tsdb.md`](../../docs/design/nyanpasu-tsdb.md). How the
crate maps onto it is in [DESIGN.md](DESIGN.md), including the canonical contracts for
[retirement](DESIGN.md#retirement-and-revocation),
[recovery and resume](DESIGN.md#recovery-and-resume),
[suspend and close](DESIGN.md#suspend-and-explicit-close),
[compaction](DESIGN.md#explicit-physical-compaction), and
[diagnostics/status](DESIGN.md#coherent-status-and-event-degradation).

Only 64-bit little-endian targets build; others fail with `compile_error!`.
