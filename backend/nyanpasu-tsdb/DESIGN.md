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
- `StorageRead::fragment` takes the most bytes its caller reserved for the fragment. A longer
  stored fragment fails `Corrupted` before it is lent, so a reader never holds more than it
  reserved, however the fragment or its directory was damaged (§7.4). redb reads the leaf into
  its page cache, where its cache budget counts it, before the length is known; the check
  drops the guard before anything borrows the value.
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
- Data fragments, blocks and large-value pieces, are at most 64 KiB − 20 bytes, well within
  §3.3's 256 KB, and the builder fills a block to 32 KiB − 32. A full fragment then fills a
  power-of-two run of redb pages, and two full blocks share one (`chunk::format`). Fragment 0
  keeps the 256 KB, which a directory of 2,048 blocks needs. Its length is recorded in the
  chunk's manifest entry, so a reader reserves what fetching it lends before it fetches it
  (§7.4).
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
- Maintenance's rounds run in this order of urgency: expire, clean up, consolidate a series
  extent, merge a bucket's late chunks, consolidate an event window, merge an extent's sorted
  runs. Expiring first keeps an extent the policy expires from being consolidated; cleanup only
  deletes and returns the disk space retention promised before a consolidation writes beside
  the old chunks; consolidation cuts the most chunks, and finishing an extent is what makes its
  leftover L0 chunks late. The tiers go before the event streams: a series query reads every
  chunk of its range, where an event scan reads chunks whose keys do not overlap as one way, so
  a window's L0 chunks, which overlap only by their late rows, cost a scan little until they
  are rewritten; and a tier's rounds are the smaller. Merging runs comes last: it rewrites
  consolidated data to keep a scan's ways within R.
- A tier is consolidated an extent at a time, oldest first, behind a cursor in its stream
  record. An extent is due once it has ended, stayed quiet for Q, and a commit has chunked its
  journal records. Once the cursor passes an extent, every L0 chunk written there is a late
  chunk, a base chunk of a bucket that no chunk covered included, and merges only with the late
  chunks of its bucket: §4.3 makes late writes delta chunks, and a base chunk there is the same
  late data.
- Consolidation keeps its bookkeeping in namespace 10 (`layout::CONSOLIDATION`): an
  index of every L0 chunk of a tier by (bucket, rows, chunk), a mark on each bucket with more than
  four L0 chunks, and the last consolidated chunk of an extent being consolidated. So planning
  reads a few records per tier, and a round takes an extent's L0 chunks in key order without
  listing the extent. `write_chunk`, the write path's way to record a chunk, keeps the index and
  the marks; maintenance's replacements and cleanup keep them too.
- A consolidation round takes whole buckets in key order while they fit F, I and its memory,
  and of a first bucket that does not fit whole as much as fits, its chunks with the fewest rows
  first, which a bucket's merge takes in any order; the rest of that bucket becomes late
  chunks. The extent's last consolidated chunk takes part again while it is not full and
  fits beside an L0 chunk, so every consolidated chunk of an extent but its last is full.
- A merge chooses among the 16 late chunks of a bucket with the fewest rows, which the L0 index
  lists first, so a round reads as much whatever the bucket's backlog. It takes as many of them
  as fit F, I and its memory and one chunk together, and writes one delta (§4.3), so it never
  rewrites a full chunk. Any two chunks of the bucket hold at least the rows of its two
  smallest, so when those do not fit one chunk, no two do. Each merge leaves the bucket a chunk
  fewer at least, so its merges end. A bucket whose two smallest late chunks hold more than a
  chunk's rows keeps them; its mark goes until a later late chunk sets it again. Late chunks
  that would fit fewer chunks without fitting one, such as three of 0.6 M rows each into two,
  stay as they are.
- A round reserves its peak from staging before it allocates, sized from its inputs' manifest
  entries: the output builder and the zstd compression context, the list of its inputs, each
  input's directory and one decoded block per column while its bucket is read, and once, the
  fragment the backend lends and what opening a chunk holds while it parses fragment 0, from
  the length of fragment 0 the entry records. Blocks decode one at a time, so the lent fragment
  counts once rather than per input (§7.4 counts it per way), as the event scan does. A round
  that reads only L0 chunks uses a decoder without a zstd context. A consolidation counts its
  inputs, then collects exactly that many; a merge chooses among a fixed 16 candidates. So no
  round's memory depends on F, and the smallest round that makes progress
  (`maintain::least_round`) depends on the schema alone: `least_staging`, the one staging
  floor opening checks, includes it. Opening also refuses F below 2 or I below twice B for a
  schema with tiers or a stream of time windows, whatever consolidates: a merge may take two
  chunks as large as chunks get, and a consolidation has to take one.
- An event stream of time windows is consolidated a window at a time, oldest first, behind the
  same cursor as a tier; a window is due once it has ended, stayed quiet for T
  (`event_buffer_age`), and a commit has chunked its journal records. Each round writes one
  sorted run (§1), chunks of consecutive ids numbered by the first, and does not take the last
  run in again: a later round's rows may come before it, which only a merge of runs puts in
  order.
- An event stream keeps its bookkeeping in the same namespace: an L0 index by (extent, first
  key, chunk), a run index by (extent, bytes, run) that records each run's chunks, and a mark
  on each extent that may hold more than R runs. The write path does not know R, so
  `write_chunk` marks the extent of every L0 chunk it writes; a round that finishes a window,
  or merges runs, sets or clears the mark by the extent's runs, and a merge round that finds R
  runs or fewer only clears it. So planning reads the stream record, the first extent after the
  cursor, and the first mark. Cleanup deletes an L0 chunk's index entry and run, and its
  extent's mark, with the chunk. A run's key holds the run's bytes, which no chunk's entry
  does, so the runs of an expired extent's consolidated chunks outlive them: once no chunk is
  left up to the watermark, cleanup deletes them, oldest extent first, within what is left of
  the round's `cleanup_chunks`, and planning finds them by the first run up to the watermark.
- A consolidation takes a window's L0 chunks by first key while they fit F, I, its memory and
  16 ways, and stops before the first that does not. Chunks whose keys do not overlap are read
  one after another as one way, chained by their first keys as the event scan chains them, so a
  window of closed connections, late by seconds, takes two ways or so however many chunks. The
  scan also chains the chunks of one sorted run where they share a key, since a group of rows
  with one key may fill more than a chunk; L0 chunks are runs of one chunk, so a
  consolidation's chaining never meets that case. A merge takes the runs of an extent with the
  fewest bytes, whole, while they fit, each run a way; each merge leaves the extent a run fewer
  at least. An extent whose two smallest runs do not fit a round keeps them; its mark goes
  until a later chunk sets it again.
- An event round's ways hold a stripe of ranks each, as the event scan's do, never a decoded
  block per column per input. The merge chooses a window of rows; the lengths of their bytes
  values decide how many the chunk being built takes (§4.3) and how many a column's copy holds
  at once; then each column is copied, each way's rows in its chunk's order through one set of
  shared decode buffers, and handed to the builder a column at a time
  (`ChunkBuilder::append_column`). A large value is copied a fragment at a time
  (`ChunkBuilder::large_value`, `ChunkReader::read_large_piece`), each input fragment becoming
  one output fragment, so the format does not change and no round holds a large value whole.
  A way opens its next chunk only after the shared buffers are let go, so a round's pool holds
  the larger of the two, not both. Its directory is sized as a planned chunk's
  (`builder::planned_directory`), since the manifest entry does not record its blocks.
- Not every profile consolidates every event stream, which §4.3 and §7.4 have every profile
  do. What a stream's rounds do depends on its spec and the profile's staging alone, so each
  round works it out per stream from the columns the schema's shape holds, with no
  allocation (`consolidates`, `compresses`), and it holds while the store is open. For
  traffic's `closed` stream the smallest round takes 1,563,275 bytes writing raw blocks and
  1,825,568 compressing:
  - staging at or above the compressing floor (desktop, x86 devices, low end): the stream's
    windows consolidate and its runs merge, compressed;
  - staging between the two floors: they do, with raw blocks, which §6.3 rule 3 allows per
    block;
  - staging below the raw floor (mobile, 1,048,576 bytes): no round of the stream is ever
    planned. The write path still marks the extent of each L0 chunk; the planner reads no
    mark of a stream it skips, and cleanup deletes a mark with its window's chunks.

  A merge reads with a zstd context whatever the profile, since a consolidated chunk may have
  been written compressed under another one, so both floors count it. Opening refuses no
  profile for them; `least_staging` does not include them.

  What mobile gives up: its windows stay L0 chunks, each a run of its own, never merged, so a
  scan merges as many ways as chunks overlap at one key, and a scan whose ways do not fit the
  query budget fails with `BudgetExceeded` (§4.3). Closed connections late by up to 30 s
  overlap only their neighbours: a day of 50,000 written every 10 s scans in 2 ways. A burst
  of late rows raises the ways; mobile's query budget scans 22 chunks of the test schema's
  `closed` that overlap at one key. Its L0 chunks also stay larger on disk: 63 bytes a row on
  redb, against 37 for consolidated compressed chunks.

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
- The backlog rule that makes a commit run the most urgent round first (§4.3, "积压").
- Consolidating epoch streams, such as core logs. Their one extent, epoch 0, never ends until
  the lifecycle gives streams epochs, and their keys are monotonic, so their runs never overlap
  and a scan reads them as one way. The planner skips them; their chunks keep the event
  bookkeeping all the same.
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
