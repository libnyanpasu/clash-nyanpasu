# Bounded current-session Core logs

Status: accepted for implementation. Delivery is a four-PR stack: subscription
level, storage retention, compression, then the browser viewer.

## Behavior

The committed Clash configuration owns the capture level. It travels in the
Clash slice of the post-commit effects plan: a changed level becomes a
`CoreLogLevel` effect, which hands the level to the streams actor by message. The
streams actor owns the logs socket and reopens only it, at the new level. A frame
the replaced socket already queued is still a record the core emitted and is
kept. Reconnects use the current level. `silent` closes the logs socket.
Changing capture level preserves saved history and never changes page filtering.
Reopening a socket cannot recover logs emitted in the gap: mihomo has no replay.

Subscribe at the capture level instead of filtering a `debug` subscription.
mihomo buffers each subscriber's events before its level filter and drops them
when that buffer is full, so a slow `debug` consumer would also lose errors.

Logs remain current-session data. A new core instance, confirmed stop, manual
clear, application shutdown or next application startup clears them. Controller
or socket reconnection alone does not clear them. Keep the existing confirmation
of stopped projections, since a late stopped projection can describe a retired
instance after its replacement is already running.

## Retention and storage

One CoreLogsActor owns bounded pending records and a redb store. Write batching
remains 64 KiB / 250 ms. The store rotates redb files at 16 MiB and evicts oldest
files to a 64 MiB total disk budget by default. Settings are application config
file options initially, not another settings panel. Sizes count actual redb
files, including indexes and dictionaries. A bounded write transaction can
temporarily exceed a threshold; eviction completes before further writes. A
deletion failure must be reported and stop growth rather than silently retain
unbounded files.

Sequences increase across shards within a generation. Rotation preserves the
generation; clearing changes it. Paging and exact-level indexes span retained
shards. Evicted cursors report CursorExpired; evicted details report RecordGone.
The frontend trims rows against the retained first/head bounds, including when
following is paused. No persisted manifest, crash recovery or old-format
migration is needed because application startup removes old session files.

## Compression

Store independent Zstd records, retaining random access. The envelope identifies
its version, encoding and original length. Compression that does not reduce size
uses an uncompressed envelope. A shard owns an immutable dictionary saved with
that shard, so changing dictionaries never prevents reading older records.

Support no compression, a bundled preset dictionary (default) and a runtime
trained dictionary. Runtime mode starts with the preset, samples at most 2 MiB,
trains at most once per session and targets a 32 KiB dictionary. The result starts
a new shard. Insufficient samples or training failure retain the preset without
a retry system. Samples and trained dictionaries end with the session. Preset
training uses synthetic messages, never private user logs.

Typed records cross the actor/store port. Validation, serialization and
compression each have one boundary; do not encode JSON then decode it to build
the level index. Decoding and keyword scans use original-size limits, including
the existing 4 MiB scan budget. Keep full-message search and UTF-8 previews.

## Viewer

Keep page-local previews bounded to 500 rows / roughly 2 MiB and one outstanding
detail request. Events carry status; consume it directly and query bodies only
for new records or requested pages. Subscribe before the initial sync, and sync
again on transport resync and visibility restoration. Coalesce arrivals while a
request runs; do not add another scheduler. Hidden pages do not query bodies.

Preserve unchanged row/view references. Merge sorted pages without full sorting.
Memoize rows with stable callbacks and avoid repeated timestamp formatting.
Keep virtual scrolling, anchors, deferred search and existing row action styling.
Bodies remain absent from shared snapshots/events and global frontend caches.

## Pre-implementation audit decisions

The read-only subagent audit identified these changes for this stack:

- Remove the repeated JSON decode and validation during append.
- Remove cursor comparisons already enforced by redb range bounds.
- Combine clear and status into one store transfer to a blocking thread.
- Cancel obsolete flush timers; do not publish unchanged empty flushes.
- Replace event + status + body + periodic polling with coalesced event reads.
- Keep unchanged views and memoized rows; linearly merge sorted pages.
- Count malformed frames as discarded records, not ordinary socket failures.
- Carry the capture level in the effects plan, not a configuration-side channel,
  and reopen the logs socket from the streams actor without a revision fence.
- Remove the streams lifecycle generation, which never changed while it ran.

Keep instance/cursor fencing, owner locks, close-before-delete, size/scan limits,
failure reporting without batch replay, and frontend disposal/resync. These
protect tested ordinary lifecycle and resource behavior, not hypothetical cases.
Do not add more actors, general session frameworks, repair workers or manifests.

## Verification

Test the level effect, silent/resume and reopening only the logs socket without
restarting unrelated streams. Test cross-shard paging, eviction, clear,
large UTF-8 records, store failures and Windows file release. Round-trip raw,
preset and trained encodings; old shards remain readable after training. Test
bounded decoding and sample collection.

Test both Unified RPC transports, cross-window clear and cursor resync. Browser
tests cover hidden views, idle reference stability, coalescing, anchors and
unmount disposal. Compare the existing page-open/streaming benchmarks twice on
the same machine, and report RPC/render counts, frame gaps, query costs and disk
size. Update docs/development/core-logs.md and regenerate bindings at each
relevant stack layer. Each PR bases on the preceding branch and is independently
testable.
