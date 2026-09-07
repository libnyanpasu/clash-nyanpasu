# Clash stream migration

## Scope and assumptions

Continue the typed Clash API migration after config writes and mode interruption.
Migrate connections, logs, traffic and memory WebSockets, including widget/UI
subscriptions. Preserve recording controls and bounded histories. Profile
interruption policy remains a separate behavior change.

Runtime FeatureSet describes controller transport availability, not the presence
of individual response fields or stream endpoints. Reuse the controller selected
by CoreActor (HTTP, Unix socket or named pipe). Decode absent extension fields as
optional and retain unknown response enum values; do not infer schemas from a
core name. A failed stream must not block the other subscriptions.

## Implementation and verification

1. Runtime protocol adapter: return owned `WebSocketStream<T>` subscriptions.
   Decode text/binary JSON, handle control/close frames, surface per-frame errors,
   preserve cross-core metadata. Verify local WebSocket fixtures, malformed frames,
   absent extensions and unknown values; update existing real-core/pipe fixtures.
2. Instance capability: handshake and every accepted sample use CoreActor's
   revocable ApiClient. Idle subscriptions are cancellable without a read timeout.
   Verify endpoint/secret/process replacement and unchanged hot patch bindings.
3. StreamsActor: own histories, recording state and subscription tasks; inject
   CoreClient. Bound producer delivery with acknowledgement, reject queued stale
   generations and cancel retry work on stop/destruction. Reset instance history
   and speed baselines when the capability changes. Verify stop/restart, stale
   frames, independent failures and bounded histories with injected test endpoints.
4. Application/UI boundary: expose operations through NyanpasuClient, migrate
   commands and widget subscription, remove direct URL/global lookup. Sequence
   snapshots/events so initial fetch, lag recovery, clear and instance reset cannot
   overwrite newer frontend state. Verify reducer ordering and binding export.
5. Delivery: runtime prerequisite PR and application PR pinning its commit. Run
   Rust tests/Clippy, generated-binding checks, frontend tests/type checks and the
   architectural ledger gate. Record actual results below before delivery.

## Lifecycle model

The protocol stream owns one transport and never reconnects or replays itself.
The application actor owns restart policy and mutable data. Workers do network
I/O outside actor handling; messages carry the capability and actor generation.
Stopping invalidates generations before task cancellation. A revoked capability
cannot publish queued data or be reused for a reconnect.

## Progress

- Runtime prerequisite: [nyanpasu-runtime #406](https://github.com/libnyanpasu/nyanpasu-runtime/pull/406), pinned at `9dbe16c`.
- Implemented owned typed streams, revocable application subscriptions, actor-owned histories/tasks, facade/IPC/widget migration, and sequenced frontend synchronization.
- Removed the application WebSocket URL builder and its direct tokio-tungstenite dependency. REST callers were migrated in the preceding PRs; no direct Clash controller transport remains in application callers.
- FeatureSet remains the controller-selection authority. Individual stream failures retry independently with a one-to-thirty-second backoff. No core-name-specific response dispatch was added.
- On core replacement, histories and connection speed baselines reset; same-instance reconnects retain history. The first connection sample establishes a baseline with zero speed, and later rates use elapsed sample time.

## Validation

- Runtime: 40 default clash-api tests and the real Mihomo HTTP/Unix-socket matrix passed; all-target/all-feature Clippy passed with warnings denied.
- Application full workspace tests passed (application library: 492 passed, one existing ignored test). A subsequent four-stream publication/socket-release test passed with the complete five-test StreamsActor suite.
- API lifecycle suite: ten tests passed, including idle WebSocket release on instance, secret, controller and shutdown revocation, while preserving a hot-patch binding.
- IPC binding export, interface/UI builds, all three frontend TypeScript checks, four frontend ordering tests (`pnpm test:clash-ws`), Oxlint, and the architectural ledger gate/33 tests passed.
- Application all-target/all-feature Clippy passed with existing warnings. No new global service, shared mutable history lock, or raw ActorRef facade was introduced.
- Windows named-pipe fixtures were updated but not executed locally. No live GUI smoke test was performed.
- The connections IPC payload retains its existing extensible JSON boundary because the current Specta exporter cannot inline recursive JSON values; decoding inside the transport is strongly typed.

## Remaining plan work

This completes the identified direct Clash API caller migration, including the frontend stream path. Profile-triggered connection interruption and precise chain selection are separate behavior changes, not remaining URL/transport callers.
