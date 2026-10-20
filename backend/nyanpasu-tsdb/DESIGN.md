# nyanpasu-tsdb design notes

The design is [nyanpasu-tsdb.md](../../docs/design/nyanpasu-tsdb.md); "§n" refers to its sections.
This file maps it onto the crate and records where the code departs from the design's sketches.

## Layout

Dependencies point down the table; a module uses only the rows below it.

| Module                                                      | Owns                                                                                        | Design            |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ----------------- |
| `db`, `lifecycle`                                           | The owner's `Tsdb` handle and the shared reader; open, recovery, suspend, close, compaction | §7.5, §9.3, §10.2 |
| `write`, `query`, `maintain`                                | Staging and publishing batches; series and event queries; budgeted maintenance rounds       | §4, §8, §9        |
| `version`, `status`                                         | The version registry; status and the lossy-operation ring                                   | §7.1, §9.4        |
| `catalog`, `head`                                           | The in-memory state, one immutable version per publication                                  | §4.2, §5          |
| `chunk`                                                     | The chunk format: builder, reader, validation                                               | §3.3, §6          |
| `codec`                                                     | Column encodings, checksums, zstd                                                           | §6.2, §6.3        |
| `layout`                                                    | Namespaces, key encodings, metadata records, the chunk manifest                             | §3.2, §4          |
| `storage`                                                   | The port, and the memory and redb backends                                                  | §3                |
| `schema`, `retention`, `profile`, `budget`, `time`, `error` | Definitions shared by everything above                                                      | §2, §7.4, §10.1   |

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
  Quiet/age head triggers fire on a write, not on maintenance: an empty batch at a later clock
  can flush a partial head and make its extent eligible for consolidation. Suspend/close only
  checkpoint it and preserve the journal; neither performs that flush or maintenance.
- A stream with more records in one extent than staging can merge at once is chunked in
  groups, each a sorted run of its own. A trigger whose chunk build does not fit what staging
  has left beside the batch waits for a later commit; head pressure and the journal cap still
  chunk. Opening refuses a staging budget below the widest chunk build plus the plan of a
  full head, so every trigger can fire.
- The owner maintains with `Tsdb::maintain(&Maintain { now, retention, rounds })`, which runs
  at most `rounds` rounds, the most urgent first, and returns `Maintained`, with `more` when
  work remains. Each round is one transaction published like a batch.
- A call that returns `more` leaves maintenance behind (§4.3, "积压"): its budget did not cover
  the work due. Until a commit finds no round due, each commit first plans one round as
  `maintain` would, less retention's rounds, which need the consumer's policy, and runs it as
  `maintain` runs a round; then it stages its batch, and `Written::maintained` reports the
  round. A round that fails with kind `BackendFailed` fails the write with
  `WriteError::Backlog`, the batch not applied, and the store needs recovery. A round that fails
  otherwise ends the backlog, and the batch commits: the consumer's next `maintain` call meets
  the failure, so a damaged chunk fails no write, and no later commit pays again for a round
  that keeps failing. The flag is held in memory, so an open starts with it clear; a call that
  finishes clears it, and a failing call leaves it as it was.

  §4.3 runs the round in the batch's transaction. Here it has a transaction of its own, before
  the batch's:
  - a non-BackendFailed round failure cannot fail the batch or roll it back;
  - the round reads the chunks it rewrites through its snapshot, and the batch stages on the
    version the round published, with no second publication number inside one transaction;
  - it reuses `maintain`'s round (`Tsdb::round`) as it is.

  It costs one more commit while behind: one more sync on a profile whose every commit is
  durable. And the bytes since the last durable commit can pass `durable_after_bytes` by one
  round's output on top of one batch, whose durability the consumer chose before the round ran.
  Without a policy, a commit's round may also consolidate an extent that the consumer's next
  call expires; that round's work is lost, and nothing else. Recycling needs no policy, so a
  commit's round may recycle, leaving out each stream whose cleanup is due.

  The rule covers a maintenance budget too small for the load. It does not cover a consumer
  that never calls `maintain`, since only a call puts maintenance behind, and §9.2 makes
  calling it the consumer's duty.

- A stream's deletion watermark is the last extent retention expired, kept in a record of the
  `STREAMS` namespace and read through the storage of the transaction or snapshot at hand,
  not held in the version. Both of retention's limits expire the oldest extent first, so the
  watermark is also the extents' expired mark: no extent carries a mark of its own. An empty
  stored extent is Corrupted: replacing its last data deletes the record instead. The same
  record counts the bytes of the stream's unexpired extents (§9.1); `manifest::replace_extent`
  and `manifest::expire` keep it exact, so a byte cap costs one read per stream.
- Reads filter by the watermark alone, at the granularity of extents. §9.1 filters by the
  later of the watermark and the current cutoff, but a query knows neither the policy nor the
  clock, so an extent that straddles the cutoff stays wholly visible until it expires whole.
- Only an extent with data expires. An empty extent before the cutoff gets no watermark of its
  own, so a late write into it is kept until the next maintenance call expires it.
- Maintenance's rounds run in this order of urgency: expire, clean up, recycle ids, consolidate
  a series extent, merge a bucket's late chunks, consolidate an event window, merge an extent's
  sorted runs. Expiring first keeps an extent the policy expires from being consolidated;
  cleanup only deletes and returns the disk space retention promised before a consolidation
  writes beside the old chunks; recycling follows the cleanup that frees ids, and comes before
  the work that only saves space or reads, since a catalog under pressure folds new
  combinations, which loses data (§5.2); consolidation cuts the most chunks, and finishing an
  extent is what makes its leftover L0 chunks late. The tiers go before the event streams: a
  series query reads every chunk of its range, where an event scan reads chunks whose keys do
  not overlap as one way, so a window's L0 chunks, which overlap only by their late rows, cost a
  scan little until they are rewritten; and a tier's rounds are the smaller. Merging runs comes
  last: it rewrites consolidated data to keep a scan's ways within R.
- Ids are recycled (§5.3) by references kept per stream and entity, not per chunk. For each
  combination and label value, a record of namespace 12 (`layout::REFERENCES`) keeps the newest
  extent of each stream whose data holds it, and namespace 13 lists the same records by stream
  and extent, oldest first. Retention deletes a stream's extents oldest first, so once cleanup
  has emptied the extents up to the watermark, a record at or below it names data that is gone.
  A stream counts as cleaned up to its watermark only while no chunk lies at or below it, so an
  expired extent keeps its ids until cleanup deletes its chunks. A stream without a watermark
  has no cleaned extent; an epoch stream's records stay, since its one extent never expires.
- The write path records the references of the points and rows a batch keeps, a late one
  dropped referencing nothing. It reduces them to the newest extent of each (stream, entity),
  reads one record each, and writes the record and moves its index entry only for a new pair or
  a newer extent, so an accepted write into an older extent records a pair the stream had none
  of. A tier's combination that the head holds in the point's bucket needs no read: its journal
  record was recorded when it was written, and a record goes only once its extent is expired,
  which takes the extent out of the head. Event buffers are not searched. Consolidation and
  merges keep the entities of an extent, so they record nothing. The list is reserved from
  staging.
- Measured on traffic's schema (`the_references_of_traffic_cost_a_read_per_new_pair`), at the
  rate of 50,000 closed connections a day written every 10 s, 5.8 rows and points a batch: with
  the event tests' 2,000 targets, nearly every connection is a new combination (10,478 in six
  hours), and the references read 11.2 records and write 19.4 entries a batch, of 47.0 reads
  and 55.1 writes in all; with 10 targets (2,508 combinations) they read 8.4 and write 4.7, of
  34.0 and 21.0. A row's combination costs a read in every batch, since event buffers are not
  searched; a point's, only when it is new to the hour.
- The live combinations that use a label value count it in namespace 11 (`LABEL_USES`): a new
  combination adds a use to each of its values but the overflow marker, and a released one takes
  it off. The counts live in storage only, so a new combination copies no catalog page for
  them; a label value's own references, in `Label` columns, are records like a combination's.
- Candidates for release are kept in namespace 14: an entity whose last record recycling
  deleted, a label value whose count a released combination took to 0, and a combination whose
  last unpin finds no record, queued in the batch that unpins. A recycling round, planned after
  cleanup and before consolidation, first deletes the records of the cleaned extents, then
  checks each candidate again (a combination live, without records and without pins; a value
  live, without uses and without records) and releases it through the round's catalog draft,
  which the round publishes. It examines at most `cleanup_chunks` index entries and candidates
  in all, stale ones included. Planning reads a candidate, then per stream its record, its first
  chunk up to the watermark and its first index entry there, however many records wait.
- §5.3 rule 4 releases a combination and the label values it leaves unused in one transaction.
  Here those values become candidates, released in a step of their own, possibly in a later
  round: nothing references them by then, and each release stays within the catalog's reserve,
  which fits one release in a fresh draft, not a combination with all its values. A round
  releases one id at a time and ends at the first its draft refuses, which waits for the next
  round; the first always fits, so every round forgets a record, drops a stale candidate or
  releases an id.
- Recycling converges once the workload stops: with finitely many writes and maintenance calls
  that succeed, the records of cleaned extents and the candidates run out. Cleanup goes first,
  so a cleanup backlog that never ends would starve it.
- A query result owns its data and outlives its snapshot, but its ids are bare numbers
  (`KeyPart::Label`, `EventValue::{Combo, Label}`) that a snapshot's catalog resolves, so after
  a release and a reuse a later snapshot names another value. §5.3 rule 5 binds the ids to the
  catalog's publication; here a `CatalogToken` binds them: a marker each open makes, an `Arc`
  compared by address that every token keeps alive, so that no two opens, of one store or two,
  share one, and a generation the catalog keeps in memory, 0 when loaded and one more for each
  version whose draft released an id. Growth keeps it, since a new id never changes what an
  old one means, so a consumer keeps rendering across batches that only add ids, where a
  publication number would change with every batch. `Snapshot::catalog`,
  `SeriesGroups::catalog` and `EventPage::catalog` return it, and a result's ids, and filters
  built from them, mean what they say through a snapshot with an equal token. Ids are not
  typed by their catalog and results do not keep it alive: the first would change every
  query's signature, the second need a second reservation for the catalog a result retains.
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
  A way opens its next chunk only after the shared buffers are let go, so its round plan covers
  the larger of the two, not both. Its directory is sized as a planned chunk's
  (`builder::planned_directory`), since the manifest entry does not record its blocks.
- Not every profile consolidates every event stream, which §4.3 and §7.4 have every profile
  do. What a stream's rounds do depends on its spec and the profile's staging alone, so each
  round works it out per stream from the columns the schema's shape holds, with no
  allocation (`consolidates`, `compresses`), and it holds while the store is open. For
  traffic's `closed` stream the smallest round takes 1,564,251 bytes writing raw blocks and
  1,826,544 compressing:
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

## Transactional diagnostics

Store format 3 requires diagnostics in namespace 15: global sequence and recycling totals at
`[0]`, cumulative counters for every tier and event stream at `[1,kind,id]`, and loss records
at `[2,seq % 64]`. Records use the ordinary format-byte and postcard framing. Only a new
dataset's durable open initializes them; reopen refuses older store formats, missing records,
invalid schema fields and inconsistent ring history. There is no migration or missing-as-zero
fallback.

Late and folded counts use input points or events, as `Written` does, even when points share
one stored aggregate. Folded field sums add accepted folded series points' field values;
mirrored events add no sums. Label overflows count accepted input event cells. Retention counts
extents and the chunk plus journal bytes their extent record already holds. Age wins a tie with
the byte cap. Only expiration counts deletions; cleanup never counts them again. Recycling
counts actual successful combination and label-value releases globally, once per lifetime.
Counters saturate; u64 input counts and field sums do not overflow in practice.

Each operation stages its diagnostics in its own data transaction. Chunking, consolidation,
opening and checkpoints never recount losses. A write appends one summary per affected stream
and kind, ordered by tier id, event stream id, then `Late`, `Fold`, `LabelOverflow`. Expiration
appends its one reason's summary; recycling is lossless and appends none. Transactions follow
execution order, including backlog rounds before a batch. Entries carry sequence, incarnation,
publication, caller time, stream, typed bounding range and deltas. The newest 64 survive;
readers visit expected sequences, not slot-key order. Rollback can reuse a sequence in a new
incarnation, so `(incarnation,seq)` identifies an entry. Exhausted sequencing is corruption.
There are no strings, catalog references, unbounded history or tracing dependency. Hosts read
these through [coherent status](#coherent-status-and-event-degradation).

Fixed records use the metadata allowance below. Write summaries have staging credit until
they are staged and dropped, before chunk planning; no head storage or profile floors change.

## Allocation ownership

Data-sized execution storage is admitted before allocation from its operation's `MemoryAccount`:
queries use the query budget; writes, opening's chunking and maintenance use staging; journal
replay uses the head budget. Shared layouts size both plans and constructors. Charged vectors and
std heaps cannot grow past leased capacity; preserving growth admits both allocations. Immutable
graphs and bounded dependency workspaces use admitted constructors, then transfer charge with the
payload. Journal and catalog label output is measured before postcard writes into fixed slices.
Chunk boundaries stream. Debug invariants check capacities against charges, live leases against
reserved credit, retained payloads against their allowances, and catalog allocations against their
layout costs. Replacement peaks include old and new storage before retirement; tests observe peaks
with retained children.

Published catalog and head versions share pages without individual claims. The catalog's resident
policy admits its loaded graph before construction and checks draft peaks and the release reserve;
the registry owns the resident head reservation. Draft copies and new head storage hold staging
credit until publication or withdrawal. Snapshots reserve capture credit before pinning a version.
Pending guards drop withdrawn graphs before returning credit. Maintenance admits one fixed round
sub-budget; its children keep the parent admission alive until their final drop.

Fixed metadata keeps the documented schema and format allowance: validation maps, bounded record
encoding, one decoded item per reader, errors, key prefixes, `Shape`, wrapper names and control
Arcs or account Rcs. Schema bounds are 65,535 labels, 16 dimensions and fields, 255 tiers and
streams, and 61 event columns per stream. Keys and metadata records have format bounds. Errors and
controls are per operation or handle, independent of stored row counts. MemoryDisk's simulated
disk payload and backend tree/transaction internals are storage and measured `backend_overhead`,
respectively; the exported conformance suite is test support. These exceptions are not an
allocator or RSS bound.

## Retirement and revocation

`Shared` owns the backend in `RwLock<Option<S>>` and the registry's admission state:
`Open`, `RecoveryNeeded`, `Recovering`, `WaitingForReaders`, `Unavailable`, `Suspended`, or
`Closed`.
Dropping `Tsdb` closes admission, sets its one atomic revocation flag, detaches the backend and
resident roots, then drops them outside the locks.
Readers own only `Shared`; snapshots own a storage transaction and a charged version.
Snapshot fields drop the transaction before the version payload and its capture credit.
Results retain their allocation charges. Even an empty version includes fixed catalog and head
allocations, so `query_budget.reserved() == 0` means all snapshots and charged results have gone.
Idle readers and catalog tokens do not block that condition. Recovery returns to the host
while claims remain; the budget has no wait API, and retirement never waits for query completion.

The lock order, documented on `Shared`, is G (backend gate), R (registry), backend locks, then
budget mutexes. Capture holds G-read and R-read through reserve-before-pin. Writes and maintenance
hold G-read, taking R briefly; no R guard spans commit, rollback or fsync.
Retirement takes G-write then R-write only to detach; backend close runs after both are released.
Queries hold neither G nor R while scanning. They check revocation at entry, successful return,
manifest entries, decoded blocks, stripe refills, large-value pieces, each event-chain link and
head/merge/page batches.
A revoked query returns its module's `CheckOpen { source: StorageError::Closed }`, kind `Local`,
and releases its workspace and partial result. The caller's snapshot retains its capture claim.
On a failed query, revocation replaces the inner error with `CheckOpen`, except `BackendFailed`,
which is still reported and returned unchanged.
Only query errors of kind `BackendFailed` latch `Open` to `RecoveryNeeded`; the next owner write
or maintenance refuses admission. Local and corrupted errors leave admission unchanged, and a late
failure cannot reopen `Closed`. An already admitted commit may finish without clearing a latch.

## Suspend and explicit close

`suspend(now)` closes admission and revokes under G-write then R-write. It releases both,
then holds G-read while rewriting the stored state record unchanged and committing Durable.
There is no new publication: `last_batch`, journal pointers and publication/head correspondence
stay intact. No maintenance or compaction runs. Finally, success or error, retirement detaches
under G-write then R-write and drops the backend and roots outside both locks. Started synchronous
commit and backend Drop I/O complete on the caller's thread; neither has a timeout.
Only a confirmed Durable commit resets durable counters and dates the last durable commit.

Suspend ends `Suspended`, including checkpoint failure. Resume uses `recover_with`, below.
`close(now)` uses the same protocol but ends terminal `Closed`. Repeated suspend/close calls do
no checkpoint and return success; close after suspend makes it terminal, and suspend after close
leaves it closed. Detached recovery states have no backend to checkpoint; stopping them succeeds
without changing durable bookkeeping. Drop still retires without a checkpoint. A host that needs
the checkpoint's outcome calls close explicitly; backend Drop itself cannot report I/O failures.
In `Suspended` and `Closed`, write and maintain return their Local `Closed` variants and capture
returns `CheckOpen { Closed }`. Sequences remain infallible and continue in the old incarnation.

## Recovery and resume

`recover_with(reopen, restore)` also resumes a suspended store. Closed returns Local
`RecoveryError::Closed` without changing state or invoking either closure. Otherwise it retires
the instance into `Recovering`, using the same unlocked
backend/root destruction as Drop. If query credit remains, it returns `WaitingForReaders` and
calls neither closure. The host releases snapshots/results and retries on its own schedule.
After drain, reopening, ordinary open validation/rebuild/settlement, and synchronous host
restoration run without either shared lock. `RecoveryView` lends auxiliary bytes and metadata
from one post-settlement snapshot; its borrowed state cannot escape. The host builds an owned
replacement and propagates read failures. It discards pending inputs computed from the old
state, never replays them. The callback's external effects cannot be rolled back.

Only success installs the backend, version and a fresh catalog marker under G then R, clears
revocation and enters `Open`. It returns `Recovered` with the restored host value, incarnation,
publication and last surviving batch. At installation, event sequence, durability bookkeeping and
`behind` reset together with the new incarnation, shape and head/staging budgets. Until then
`next_event_seq` continues in the old incarnation; the other owner bookkeeping remains unchanged.
`Shared`, the query budget and existing Reader clones survive. Old graphs drain before new ones
are built; old query failures cannot reach the new instance.

Any recovery error drops the candidate outside the locks and leaves `Unavailable`, with no backend
or published version. The host receives the typed cause and may retry explicitly; durable opening
changes may already have happened. `RecoveryNeeded`, `Recovering`, `WaitingForReaders` and
`Unavailable` return `RecoveryNeeded` from write, maintain and capture. Revoked running queries
retain their own Local `CheckOpen` errors. Every state except `Closed` can start another recovery
attempt. There is no automatic retry or wait.
Reopening must target the same dataset; recovery from an old backup needs the deferred external
incarnation counter.

## Explicit physical compaction

`CompactStorage` is an optional lifecycle capability, separate from transactional `Storage`.
`Tsdb::compact()` exists only for backends implementing it. MemoryStorage returns `Unsupported`;
RedbStorage maps its existing Compacted/Unchanged outcomes to `Executed { changed }`, and a live
read transaction to `Deferred`. Other backends must likewise defer if shared backend ownership
prevents exclusivity. There is no facade snapshot counter or query-budget drain check: idle
Readers and results own no storage snapshot, and do not defer compaction.

The owner takes G-write, checks admission briefly under R, then releases R before calling the
backend. Captures wait on G and proceed afterwards. Compaction changes neither publication nor
revocation. Existing healthy queries keep working after a Deferred attempt. Only Open admits it;
recovery states return `CompactError::RecoveryNeeded`, and Suspended/Closed return Local Closed.
BackendFailed flags recovery; Local and Corrupted errors leave the state unchanged. No registry
lock spans compaction's durable work. The caller decides when to retry Deferred; there is no
polling or duration bound. Suspend, close and maintenance never invoke compaction.

## Coherent status and event degradation

`Tsdb::status()` and `Snapshot::status()` share one implementation. The facade captures one
storage transaction and its matching version through G then R; readers can use a capture. Reads
release both locks before scanning. The snapshot also captures the single ephemeral `behind`
flag under R. Outside Open, status returns lifecycle and query-credit usage without dataset
values; a retired snapshot never exposes its old totals as current. BackendFailed flags recovery;
Local admission errors and Corrupted metadata do not.

Status makes one manifest metadata pass per stream, plus head rows, catalog ids, pins,
persistent marks/runs and recycle candidates. It decodes no chunk payload, takes no clock or
retention policy and runs no maintenance. Live rows sum each unexpired `ChunkEntry.rows` and
head accumulator combinations or event rows: overlapping aggregate chunks count separately.
Bytes sum chunk fragments and head journal bytes including journal keys. Bounds are actual
bucket numbers or event keys, not extent boundaries. Expiration reports the last expired extent;
downsampling is absent. Expired chunks and their bytes appear only as pending cleanup.

E7 counters keep their input units. Catalog sizes exclude implicit id 0; its presence and pins
are explicit. Resident folded combinations contain overflow labels; released totals count actual
recycling. Persistent work counts are observations, not predictions of rounds due. Ring entries
are the surviving, sequence-ordered 64 records. The result retains only owned scalars and arrays:
one element per schema stream and at most 64 ring entries, charged to the query budget until
drop. It keeps no backend transaction, catalog token or version; workspace credit returns before
return. Query reserved bytes are an observation of concurrent usage at return.

`EventPage::folded()` reports examined folded event rows and individual overflowing Label cells
before filtering, even when predicate statistics reject a block. Counting then reads only its
Series/Label ids and any boundary/cursor ranks, preserving other payload skips. Null cells do
not count. Rows outside the range or before the cursor do not count; head selection and chunk
prefetch can examine past a page limit, so these are per-scan counts, not distinct across pages.
`SeriesGroups` metadata is unchanged. No stored record or format version changes for status.

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

| Operation             | Error type                                                                                          |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| validating a schema   | `schema::SchemaError`                                                                               |
| opening               | `lifecycle::OpenError`                                                                              |
| recovering            | `lifecycle::RecoveryError`                                                                          |
| suspending or closing | `lifecycle::CheckpointError`                                                                        |
| compacting            | `lifecycle::CompactError`                                                                           |
| writing a batch       | `write::WriteError`, with `InputError`, `StageError` and, from the backlog's round, `MaintainError` |
| capturing a snapshot  | `version::CaptureError`                                                                             |
| summing series        | `query::SeriesQueryError`                                                                           |
| scanning events       | `query::EventScanError`                                                                             |
| reading status        | `status::StatusError`                                                                               |
| maintaining           | `maintain::MaintainError`                                                                           |

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

## Verification notes

`verification::faults::FaultSchedule` records instance-local ordinals, then replays each
operation from a fresh durable fixture with a fault and, separately, a crash at every ordinal.
Port probes reuse the lifecycle harness; disk schedules reuse the conformance generation and
redb's FaultDisk. The oracle compares exact bytes across all namespaces and all physical
fragments, including orphan fragments. One-round operations admit only the before or committed
image. Reached `port.commit.after` boundaries, including the armed ordinal, select the exact
prefix: none means before; one means the first round for two-round operations; two means final.
NotApplied cannot advance that prefix. Before the first reached `disk.sync.after`, a disk
schedule must retain the before image. Reported I/O errors are BackendFailed and latch a
surviving instance; rollback Drop I/O is checked through that latch. Backend-close Drop cannot
return I/O errors. redb can absorb a best-effort cache-eviction write failure by buffering
and flushing it at commit; that case must be committed and remain usable. Reopen advances the
incarnation; recovery restores auxiliary state without replaying input. Weak budget probes and
held root-budget handles check that teardown and the last old result return their charges.

Hooks outside the port cover root memory admission, catalog-load admission, catalog/head
rebuild, and pending registration/publication. Zero-byte claims and transfers of prepaid
sub-budget credit cannot fail admission. Injected admission pressure lasts through the attempt's
fallback allocations; recycling fixtures bound a transaction to one candidate. Storage failures
latch the actual memory backend. Infallible in-memory boundaries stop through a private unwind
payload; ordinary panics are never swallowed. Every armed ordinal must fire and the reached
trace must match its recorded prefix.

A crash freezes the durable image by invalidating the backend generation before unwinding or
Drop. redb schedules fail read, write, set_len, sync (before and after applying it) and close,
including I/O from rollback and compaction. Real-file barriers park each allocator/header close
sync, check that G and R are free while the file remains locked, then release and join before
asserting. These are in-process durable-image and lock tests, not subprocess kills or proof of
hardware power-loss behavior. Those tests remain out of scope. Fixtures are synthetic and
exhaustive, never sampled.

The label-keyed cross-feature model keeps current and durable states, queries decoded public
values and RecoveryView, and resolves Unknown through last_batch. Watermarks are monotone; byte
caps are checked in both directions against public status bytes, and age retention converges by
whole extents. Predicted write losses are checked against the ring in order, including its suffix
when full. Fixed counterexamples cover chunk ordering, late deltas, zero sums, auxiliary rollback,
sequence contracts and old-snapshot behavior. Proptest uses 64 cases in normal gates; a separate
512-case run passed at E10. Budget teardown checks require zero when nothing remains held; exact
peak measurements belong to the ignored release harness, not a tautological peak<=limit test.

Platform results, provisional profile evidence, measurement commands and hardware limits live
once in [BENCHMARKS.md](BENCHMARKS.md#e13-profile-evidence-and-verification-matrix). These tests
are not verification of iOS lifecycle integration or actual power loss.

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
- Consolidating epoch streams, such as core logs. Their one extent, epoch 0, never ends until
  the lifecycle gives streams epochs, and their keys are monotonic, so their runs never overlap
  and a scan reads them as one way. The planner skips them; their chunks keep the event
  bookkeeping all the same.
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
| `snafu`             | Domain errors with context selectors, as elsewhere in the workspace                 |
| `strum`             | Enum tags and variant lists derived from the definitions                            |
| `serde`, `postcard` | The stable byte form of structures, such as the schema fingerprint's input          |

Each dependency is added by the change that first uses it.
