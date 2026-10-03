# Application updater

The application updater separates checking, downloading, verification, and
installation. It is independent of the Clash core updater.

## User behavior

- About has a version card containing update status and operations, plus separate
  cards for the release channel, package download sources, and automatic updates.
  Each card uses its content height; the version card retains the centered logo
  and version layout.
- Manifest endpoints follow the selected release channel and are not displayed
  in the frontend. Package download sources use drag-and-drop priority ordering.
- Automatic checking retains its existing default. Automatic package downloads
  default to disabled and require automatic checking to be enabled.
- Background downloads do not open a dialog or navigate to About. Closing a
  dialog, leaving About, or destroying a webview does not cancel a download.
  A ready package appears in the header's update indicator and About; background
  work does not request notification permission or display a blocking dialog.
- Download cancellation is explicit. `cancelling` means cancellation has been
  requested; `cancelled` means the operation has ended and released its data.
- A cancelled release is not immediately downloaded again by automatic work.
- Installation always requires an explicit user action. It is not cancellable
  once the platform installer takes over.
- Downloaded packages survive page changes, but not application termination.
  There is no persistence or partial-download resume in this implementation.

## Ownership and boundaries

`NyanpasuClient` exposes application update operations through `AppUpdateClient`.
`AppUpdateActor` owns the current release, phase, progress, cancellation handle,
and prepared package. Infrastructure and platform installation use injected
ports. No update resource belongs to a webview resource table.

The owner supervises a single operation at a time. A download operation can wait
for network IO while the update owner remains available for cancellation and
snapshot queries. Explicit cancellation ends the network future; losing an RPC
waiter does not cancel owner-started work. Operation identities prevent late
progress or results from replacing the state of a newer operation.

The application configuration owner supplies committed update settings. The
frontend does not schedule checks or downloads. The application bootstrap wires
the ports and lifecycle; the updater does not obtain dependencies from globals.

Snapshots carry a monotonically increasing revision. The query consumer listens
for update events, fetches the initial snapshot, and refetches on transport
resynchronization. It rejects older snapshots so a delayed read cannot replace
newer progress. All six updater RPC operations remain desktop-only; only the
snapshot read is a query. Checking is a mutation because it changes owner state.

## Verification and installation

The Tauri updater plugin's download-finished callback runs before signature
verification. It moves the UI to `verifying`, not `ready`. Only a successful
download result supplies a prepared package. Every mirror attempt preserves
the release version, signature, public key, and platform target.

The platform adapter reuses Tauri's installer and the application's existing
shutdown/restart boundary. Installer handoff must not enter the same shutdown
wait it is itself preventing from completing.

The current plugin returns the entire verified package as bytes. This keeps the
first implementation small, but memory use includes the complete package.

## Future package persistence

`PreparedAppUpdate` retains the checked release and platform context;
`VerifiedAppUpdate` carries the verified bytes. Future persistence belongs behind
these adapter boundaries, rather than in About or RPC resource IDs. It requires
a durable release identity, verification of cached bytes, and restoration of the
platform installation context.

Tauri updater 2.13 does not expose a serializable `Update` or a public constructor
for restoring its private installation context. Writing bytes to disk alone
does not provide safe installation after restart. Persistence remains deferred;
there is no unused cache implementation or promise of offline restoration.
