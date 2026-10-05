# Repository automation

Source is grouped under `src/`; tests live beside their modules. Runtime
settings and the dependency lockfile live here, while the root `deno.jsonc`
defines all public commands. Run `deno task` from the repository root to list
them.

See the [development guide](../docs/development/scripts.md) for categories, task
conventions, dependency handling and verification.

```sh
deno task prepare:check
deno task generate:git-info
deno task lint:deno
deno task lint:frontend-boundaries
deno task test:scripts
```

## Nightly build eligibility

Scheduled and manual nightly runs first execute
`deno task check:nightly-changes`. The check compares the `pre-release` tag's
tree with the run's checkout, excluding `docs/`, Markdown/reStructuredText
files, `tests/` and `__tests__/` directories, and files named `*_test.*`,
`*.test.*` or `*.spec.*`. Other changes, including core version manifests,
dependencies, submodule revisions, resources, build scripts and workflows,
trigger the six platform builds. A fully reverted change does not.

If there are no build input changes, package builds, tag/release updates,
uploads, updater publication, archive verification and Telegram notifications
are skipped. The check reports its decision in the Actions summary. A missing
`pre-release` tag allows the first build; Git failures fail the check. The tag
is the existing nightly baseline and is moved after all package builds pass; if
later publication steps fail, rerun those failed jobs to complete publication.

## Updater distribution

`deno task updater` publishes the latest stable and beta feeds. The nightly feed
is published by `deno task updater:nightly`. The release workflow copies these
manifests to `nyanpasu.surge.sh`. Surge currently hosts manifests only; package
downloads use the sources selected in the app's About settings: Nyanpasu, GitHub
Releases, GHFast, or SourceForge. GHFast and SourceForge are optional and do not
change saved source order. Its URLs prefix the complete GitHub URL with
`https://ghfast.top/`; Nyanpasu uses the GitHub path without its host. Every
package source retains the same release signature and updater public key.

The SourceForge web site is a separate, optional manifest host. Its manifests
are published as static JSON at `https://<project>.sourceforge.io/updater/`;
binary files stay on SourceForge FRS under immutable
`nightly/<run>-<attempt>-<sha>` or `releases/<tag>` directories. The app
receives the Project Web endpoint at build time only when that host is enabled.
Surge and GitHub feeds remain available independently.

## Binary mirrors and archive uploads

GitHub Releases remain the primary publisher. After all six platform builds
finish, the central storage workflow downloads their finalized GitHub artifacts,
validates the complete target set and unique asset names, then publishes the six
target inventories to configured mirrors with one shared UTC timestamp. Setting
`SOURCEFORGE_PROJECT` automatically connects FRS mirroring, Project Web manifest
publication, and the compiled app fallback endpoint. It also requires the
`SOURCEFORGE_USERNAME` variable (or secret) and the `SOURCEFORGE_SSH_KEY` and
`SOURCEFORGE_KNOWN_HOSTS` secrets. There are no separate enabled flags. The
known-hosts value must cover both `frs.sourceforge.net` and
`web.sourceforge.net` for FRS and Project Web. The publication workflow verifies
every public file's size and SHA-256 before promoting mirror metadata.

Release uploads also read the existing GitHub `sourceforge-mirrors.json` before
writing to FRS. Recorded files must keep the same size, SHA-256 and mirror URL;
lookup failures stop the upload. CI supplies `GITHUB_TOKEN` and
`GITHUB_REPOSITORY` for this check. Manual release uploads require those values
as well. Before uploading release bytes, the uploader also reserves each file
using an exclusive SFTP directory under `releases/<tag>/upload-inventory/` and
records its size and SHA-256. Retries must match that write-once inventory even
when a previous attempt failed before attaching the GitHub sidecar. Identical
retries can finish partial uploads; conflicting bytes are rejected. Release and
backfill workflows share a concurrency group to serialize their writers. If a
connection fails between creating a reservation and saving its inventory, the
uploader fails closed. Inspect the FRS file and reservation before repairing the
missing or incomplete inventory; do not remove a claim for published bytes.

Updater manifests are published on SourceForge Project Web using the same
project and SSH values. The workflow stages each feed and renames it only after
all files have uploaded; it verifies the public JSON before the nightly cleanup
job can run. It only adds `updater/index.html` when that file is absent.

Release files reserve their immutable size/SHA-256 inventory under
`releases/<tag>/upload-inventory/<filename>/inventory.json` before uploading.
SourceForge forbids dot-prefixed file and directory names, so this directory
must not start with `.`. An unreadable reservation fails closed and reports both
its creation error and its read error; do not remove the reservation to bypass
an immutable-byte conflict.

Nightly and release builds validate all six target inventories before storage
writes. SourceForge and Telegram publish in separate jobs with independent
writer locks. GitHub updater generation starts after GitHub asset upload and
does not wait for storage. A subsequent updater job attaches only verified
SourceForge metadata and publishes those feeds to SourceForge Project Web.
SourceForge preflight errors are reported without blocking package builds or the
GitHub updater.

All finalized packages, updater bundles and signatures are uploaded as documents
to `@ClashNyanpasu` using the historical MTProto/GramJS approach. Configure
`TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_TOKEN` and an archive
registration token (`ARCHIVE_UPLOAD_TOKEN`, `FILE_SERVER_TOKEN` or
`UPLOAD_TOKEN`). The bot must be allowed to post documents to the channel.
Documents upload sequentially with eight concurrent part workers per file. No
Bot API download URL or token is exposed. The archive lists Telegram files and
redirects `/bin/:id` to the corresponding
`https://t.me/ClashNyanpasu/<message-id>` post. This is a Telegram message link;
users download the document through Telegram.

Deploy `nyanpasu-file-list` migration `0004_add_telegram_storage.sql` and its
Telegram-aware routes before enabling this uploader. Old IA and OneDrive rows
remain readable; matching IA copies are hidden when their Telegram replacement
is indexed. New builds no longer upload to IA, and IA credentials are not
required by release/nightly publication. Historical IA tools remain available
for already published data. The retained item prefix only preserves old manifest
identities during recovery; it does not enable IA publication.

SourceForge uploads six targets concurrently and public verification checks four
files concurrently. Normal publication hashes the mirror once in its independent
verification job. Nightly cleanup requires archived copies of all six targets
(Telegram, or legacy ready IA entries) before deleting an old SF folder. Missing
credentials or archives retain the folder. Releases are never pruned.

### Debug and recover without rebuilding

Use `[Maintenance] Debug and Recover Storage Publication` with the completed
package run id and `backend=telegram`, `sourceforge` or `both`. `mode=upload`
uses retained signed packages without rebuilding. The six-target inventory,
original run/attempt/commit/timestamp and every size/hash are checked before any
remote write. `mode=preflight` checks configuration, archive auth/database
access and optional SourceForge SFTP connectivity without posting documents.

Telegram checkpoints each uploaded document's message id, document id, size and
hashes in `telegram-report.json`. For subsequent recovery, set
`telegram_report_run_id` to the run containing that report; the default is the
original source run. Recovery checks saved messages and skips already uploaded
documents. `mode=register` retries archive indexing from saved receipts without
uploading bytes; `mode=verify` checks saved channel messages without uploading
or registering. If the first Telegram transfer has no report, use `mode=upload`.
The report remains available even if later uploads or archive registration fail.
An expired or conflicting receipt fails closed rather than posting a duplicate.

For the failed release run `37342393456`, choose that `source_run_id`,
`mode=upload`, `backend=telegram`, `target=all` after deploying the archive
change and pushing the main repository changes. This transfers its existing
signed packages to the channel. Recovery does not publish updater feeds:
manually run `[Reusable] Publish Updater Manifests` with `nightly=false` to
regenerate the GitHub/Surge feeds; pass verified SF metadata only after SF
recovery succeeds. Re-running an old release job uses its original SHA, not
these new scripts.

Locally, use
`deno task telegram:publish --publication-dir <dir> --report
<report.json>`; an
existing report is reused and must belong to the same inventory. Add
`--target <target>`, `--register-only` or `--verify-only` for scoped recovery.
SF-only recovery verifies public hashes and saves a complete
`recovery-reports/sourceforge-mirrors.json` for the updater workflow. Partial
mirror metadata must not replace the complete six-target mirror inventory.

Use `[Maintenance] Backfill SourceForge Release Mirror` for an existing
published release tag when Actions artifacts are no longer retained. It
downloads GitHub release assets, verifies SF bytes and attaches mirror metadata;
it does not rebuild packages or publish an updater feed.

## Legacy OneDrive upload diagnostics

These diagnostics apply to historical OneDrive publication runs and the legacy
upload tools. Current publication workflows keep OneDrive package uploads
disabled.

If a report contains `quotaLimitReached` (sometimes wrapped in HTTP 500) or HTTP
507, the archive's backing OneDrive storage has exhausted its quota. The
uploader stops retrying that file and the final verification summary includes
the file name, upstream error, and recovery advice. Network and transient server
failures retain bounded retries. A quota failure still fails archive
verification; it does not count as a successful upload or allow the archive
notification.

Check storage usage on the backing drive, remove obsolete nightly archives under
an agreed retention policy, or increase capacity before retrying. Monitor free
space and reserve enough for all platforms, including fixed-WebView bundles.
Reducing chunk size or increasing retry counts does not add storage capacity.
See Microsoft's
[OneDrive error codes](https://learn.microsoft.com/en-us/onedrive/developer/rest-api/concepts/errors?view=odsp-graph-online).

GitHub Actions package artifacts and GitHub Release publication are independent
of archive verification and remain fallback downloads when their steps
succeeded. Re-running a historical `Verify Archive Uploads` job rechecks the
same reports; it does not upload missing files. After capacity is restored,
retry the affected upload from the retained artifacts into its original
`FOLDER_PATH`, or start a fresh nightly build. Avoid rebuilding all platforms
merely to diagnose a storage quota error.

## Telegram notifications

Telegram notifications contain release/build information and the corresponding
GitHub Release download page (`pre-release` for nightly builds). Notifications
are available through manual dispatch; normal release/nightly jobs upload files
through `telegram:publish`. The manual notification does not download or upload
packages. To resend a release notification after the updated workflow is
available on GitHub, manually run `[Reusable] Notify Telegram of Releases` with
`nightly: false` and the published `tag`, for example `v2.0.0-beta.1`. This
sends only the notification to `@keikolog`; it does not rebuild packages.
Re-running an older failed publication run uses that run's original workflow and
scripts.
