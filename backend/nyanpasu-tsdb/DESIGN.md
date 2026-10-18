# nyanpasu-tsdb design notes

The design is [`docs/design/nyanpasu-tsdb.md`](../../docs/design/nyanpasu-tsdb.md); "§n" refers to its
sections. This file maps it onto the crate and records where the code departs from the
design's sketches.

## Layout

Dependencies point down the table; a module uses only the rows below it.

| Module                                                      | Owns                                                                                  | Design            |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------- |
| `db`, `lifecycle`                                           | The owner's `Tsdb` handle and the shared reader; open, recovery, suspend, compaction  | §7.5, §9.3, §10.2 |
| `write`, `query`, `maintain`                                | Staging and publishing batches; series and event queries; budgeted maintenance rounds | §4, §8, §9        |
| `version`, `status`                                         | The version registry; status and the lossy-operation ring                             | §7.1, §9.4        |
| `catalog`, `head`                                           | The in-memory state, one immutable version per publication                            | §4.2, §5          |
| `chunk`                                                     | The chunk format: builder, reader, validation                                         | §3.3, §6          |
| `codec`                                                     | Column encodings, checksums, zstd                                                     | §6.2, §6.3        |
| `layout`                                                    | Namespaces, key encodings, metadata records, the chunk manifest                       | §3.2, §4          |
| `storage`                                                   | The port, and the memory and redb backends                                            | §3                |
| `schema`, `retention`, `profile`, `budget`, `time`, `error` | Definitions shared by everything above                                                | §2, §7.4, §10.1   |

Only the port (`storage`), the definitions and the facade are public; the rest is
internal, so the format and the in-memory structures can change without breaking
consumers.

## Departures from the design's sketches

The design says its code sketches show shape, not the final API. Where the code differs:

- Errors follow snafu's design instead of the single `ErrorKind`-tagged error of §3.2
  (see [Errors](#errors)). `ErrorKind` remains, as the `kind()` of every error type.
- `StorageRead::scan` returns the value its callback broke with, so a decoding error can
  stop a scan and come back out.
- `StorageWrite::commit` returns `Result<(), CommitFailure>`; there is nothing a receipt
  would carry.
- `ColumnType` splits `Int` into `I64` and `U64`, and adds `FoldedLabel`: the column of
  §11.1 that keeps an event's own value of a folded dimension. The store fills it, because
  only staging knows whether the fold ladder applied.
- `Retention` has no `coarsen_after` yet. It arrives with the day tier, an extension
  (§13).
- Checksums use CRC32 through `crc32fast`. The crate is already in the workspace and
  detects the hardware instruction at runtime, which is what §6.1 asks of XXH3 or CRC32C.
- A consumer writes a batch with one call, `Tsdb::write`, which stages, registers, commits
  and publishes it. The store owns the transaction, so the consumer never holds it across its
  own code (§4.4 sketches a stage and a commit around a transaction the consumer opens).
- A pin is named by a key the consumer chooses (`PinOp::Pin { key, labels }`,
  `PinOp::Unpin { key }`), stored with the combination it pins in the store's `PIN_KEYS`
  namespace. The consumer's auxiliary state refers to its own key in the same transaction, and
  an unpin never resolves labels, which could name another combination after a fold or a
  change of the catalog budget (§5.3).
- Journal records are framed: a header, then one postcard item per row or per combination.
  Chunking and opening read them one item at a time, so their memory does not grow with a
  record's rows. Event records are sorted by key, incarnation and sequence, and are kept
  per stream and extent, so that a record's bytes belong to one extent (§9.1).
- An extent is numbered: its bucket or key divided by the extent width. A start time could
  overflow at the edges of `i64`.
- In an event chunk, `FoldedLabel` and `Identity` columns are low-cardinality bytes. An
  identity is a one-byte tag followed by its 16 bytes or its text.
- An event stream has at most 61 columns, because its chunks add the key, the incarnation and
  the sequence.
- A commit plans from sizes before it builds anything: it reserves the plan's peak from
  staging, builds only the sources the head keeps, and writes a chunked source straight from
  the current pages and the batch's deltas. Staging resolves a batch's event rows in the
  order chunks keep them.
- A stream with more records in one extent than staging can merge at once is chunked in
  groups, each a sorted run of its own. A trigger whose chunk build does not fit what staging
  has left beside the batch waits for a later commit; head pressure and the journal cap still
  chunk. Opening refuses a staging budget below the widest chunk build plus the plan of a
  full head, so every trigger can fire.
- The owner maintains with `Tsdb::maintain(&Maintain { now, retention, rounds })`, which runs
  at most `rounds` rounds, the most urgent first, and returns `Maintained`, with `more` when
  work remains. Each round is one transaction published like a batch.
- A stream's deletion watermark is the last extent retention expired, kept in a record of the
  `STREAMS` namespace and read through the storage of the transaction or snapshot at hand,
  not held in the version. Both of retention's limits expire the oldest extent first, so the
  watermark is also the extents' expired mark: no extent carries a mark of its own. The same
  record counts the bytes of the stream's unexpired extents (§9.1); `manifest::replace_extent`
  and `manifest::expire` keep it exact, so a byte cap costs one read per stream.
- Reads filter by the watermark alone, at the granularity of extents. §9.1 filters by the
  later of the watermark and the current cutoff, but a query knows neither the policy nor the
  clock, so an extent that straddles the cutoff stays wholly visible until it expires whole.
- Only an extent with data expires. An empty extent before the cutoff gets no watermark of its
  own, so a late write into it is kept until the next maintenance call expires it.

## Errors

Errors follow snafu's own guidance (`guide/philosophy.md`).

- **Each module has its own error types.** A caller matches only the failures of the
  operation it called. A query, for example, never has to handle a stale batch id.
- **A variant names the step that failed** and carries that step's data. One underlying
  error therefore appears under several variants, one for each step it can interrupt:
  `ReadFragment { id, source: StorageError }` and `CommitBatch { source: StorageError }`,
  rather than one `Storage { op: &str }`.
- **The port has its own `StorageError`.** It is the contract that backends map their
  failures into: failed, corrupted, beyond a limit, closed. The store's modules wrap it
  with the step that hit it.
- **Every error type has `kind() -> ErrorKind`**, delegating to its source. This is the
  recovery decision of §7.5:
  - `Local`: end this operation;
  - `BackendFailed`: reopen the backend;
  - `Corrupted`: the data is damaged.
- **Public error types are enums, not opaque.** The application has to tell some causes
  apart, such as an expired cursor or an exhausted budget. The enums are
  `#[non_exhaustive]`.
- **A failure shared by several modules is defined once, as a source**: `BudgetExceeded`
  in `budget`, chunk decoding errors in `chunk`. Each caller wraps it in its own step,
  for example `ReserveScan { source: BudgetExceeded }`.

Each public operation returns the error type of its module:

| Operation            | Error type                                              |
| -------------------- | ------------------------------------------------------- |
| validating a schema  | `schema::SchemaError`                                   |
| opening              | `lifecycle::OpenError`                                  |
| writing a batch      | `write::WriteError`, with `InputError` and `StageError` |
| capturing a snapshot | `version::CaptureError`                                 |
| summing series       | `query::SeriesQueryError`                               |
| scanning events      | `query::EventScanError`                                 |
| maintaining          | `maintain::MaintainError`                               |

## Data mapping and variants

A mapping between a type and its bytes, or between an enum and its variants, is derived
from the type's definition by a library. It is never written out by hand, so it cannot
drift from the definition.

- **An enum stored as a tag** is `#[repr(u8)]`, and every variant has an explicit
  discriminant that is never renumbered. The tag is written with `as u8` and read back with
  `strum::FromRepr`. An unknown tag is a corruption error.
- **A list of variants** comes from `strum::VariantArray` or `strum::EnumIter`, never from a
  hand-written array. When the order means something, such as a tie-break between
  encodings, the variants are declared in that order and the enum says so.
- **A structure that has to keep a stable byte form**, because it is stored or hashed into
  a stored value, derives `serde::Serialize` and is encoded with postcard, whose 1.x wire
  format is specified and stable. The schema fingerprint is FNV-1a over that encoding.
  Within such a record, the order of its fields and the order of an enum's variants are
  part of the format. postcard writes a variant as its index. So an enum that carries data
  derives serde too, and is never reordered, just as a tag is never renumbered. A golden
  fixture of the record's bytes pins that order.
- **std's `Hash` is never stored.** The std docs ("Portability" under `Hash`) state that
  the data `Hash` feeds a `Hasher` is not portable across platforms and, for most standard
  types, not stable between compiler versions. The workspace builds on nightly, so a
  toolchain update could change a stored fingerprint and lock users out of their data. The
  in-memory hash tables (the catalog's dictionaries) do use `Hash`. They are rebuilt on
  every load and keyed with random SipHash keys, which is what `Hash` is for.

Crates offering a stable hash trait were considered and rejected:

- `stable-hash` 0.4.3 (graphprotocol) has a stable hash trait, but its last release was in
  2022-11. It brings blake3, ibig, uint and lazy_static, and has no derive.
- `rustc-stable-hash` only provides rustc's hasher. rustc's stability comes from its
  internal `HashStable` trait, which is not published.
- `deterministic-hash` fixes endianness and `usize` width, but not the encoding of std's
  `Hash` impls.

`serde::Serialize` is the derived trait instead, and postcard pins its encoding.

## Not in this implementation

- The query result cache (§7.3), until benchmarks show the need.
- The compact catalog representation (§5.1): column widths chosen per dimension. It waits
  for a measurement on the mobile profile.
- The extensions of §13: the day tier, catalog paging, the segment-file backend, trained
  dictionaries, LZ4, n-gram filters, and a hard bound on merge fan-in.
- Merging catalog records into catalog chunks (§5.1). The catalog is stored as one record
  per value and per combination, and is loaded by scanning them.
- Compacting a dictionary's heap. Released values leave holes until the next load, so a
  long-running gateway with heavy label churn grows its heap until it restarts.
- Stream epochs (§1, §8.3). Until the lifecycle defines them, every extent of an
  `ExtentSpec::Epoch` stream is epoch 0.
- Retention of epoch streams. Expiring an epoch stream's one extent would drop every later
  row of it too, and `max_age` cannot date sequence keys, so `maintain` refuses a policy that
  limits one (`MaintainError::EpochRetention`).
- Splitting event extents by size under a byte cap (§9.1, "单个区段不超过字节上限的八分之一").
  A cap expires whole time windows, so a cap below one window's bytes expires the newest
  window too, and its later rows are dropped as late until the next window starts.
- The lossy-operation ring, `status()` and the logs of lossy maintenance rounds (§9.4).
- Recycling the combinations and label values that only expired extents referred to (§5.3).
- The incarnation counter kept outside the database for tmpfs hosts (§8.3), which only the
  untested tier-2 profile needs.
- Consumers: traffic, core logs and job logs, and migration of the traffic database.

## Dependencies

| Crate               | Why                                                                                 |
| ------------------- | ----------------------------------------------------------------------------------- |
| `redb`              | The first backend (feature `redb`)                                                  |
| `zstd`              | The C zstd, cross-compiled for every tier-1 platform, and already used by core logs |
| `crc32fast`         | Checksums                                                                           |
| `zerocopy`          | Typed little-endian views of validated column blocks, with no `unsafe`              |
| `rustc-hash`        | Hashing integer keys. Strings that outsiders control use the standard SipHash       |
| `tracing`           | Logs of lossy operations (§9.4)                                                     |
| `snafu`             | Domain errors with context selectors, as elsewhere in the workspace                 |
| `strum`             | Enum tags and variant lists derived from the definitions                            |
| `serde`, `postcard` | The stable byte form of structures, such as the schema fingerprint's input          |

Each dependency is added by the change that first uses it.
