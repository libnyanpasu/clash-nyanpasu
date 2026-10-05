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
using an exclusive SFTP directory under `releases/<tag>/.upload-inventory/` and
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

Nightly and release builds first validate the complete six-target inventory and
retain its original publication timestamp. SourceForge and IA then publish in
separate jobs, each with its own `storage-publication-<backend>` writer lock and
report artifact. Both jobs download the original signed build artifacts and use
the shared timestamp; no additional package bundle is uploaded between jobs.
Updater publication waits for verified SourceForge hashes and does not wait for
IA. Normal SourceForge publication delegates public hashing to that independent
verification job instead of downloading the inventory twice.

Both backends process at most two targets simultaneously. Each IA target creates
its item with the manifest first, then uploads at most two package files at a
time (four concurrent transfers across targets). The independent SourceForge
public SHA-256 verification job downloads at most four files simultaneously;
manual recovery can run two such target verifications concurrently. Failed tasks
are collected after the other queued targets finish, preserving their reports.

Setting `IA_ITEM_PREFIX` automatically connects Internet Archive archiving.
Configure `IA_ITEM_PREFIX` as a repository variable. The uploader lives in this
repository and runs from the same checkout as the publication workflow.
Configure `IA_ACCESS_KEY`, `IA_SECRET_KEY`, `IA_UPLOADER` and the existing
`FILE_SERVER_TOKEN` secrets. `IA_UPLOADER` must exactly match the value sent as
IA `metadata.uploader` (often the account email, not the public username).
Configure the Archive Hub Worker with the same `IA_ITEM_PREFIX` and
`IA_UPLOADER`, then apply migration `0003` and deploy the Worker before adding
the CI `IA_ITEM_PREFIX` variable. Archive ingest may remain pending after a
successful byte upload; that status is reported separately and does not hold up
SourceForge or updater-feed publication. To reconcile a pending item after IA
ingest, use the main repository's
`deno task archive:verify --build-id <target-build-id>
--server https://archive.nyanpasu.org --report <report.json>`.
Old nightly FRS directories are pruned only after manifest publication succeeds
and the corresponding IA archive builds report ready. Release directories are
retained indefinitely.

Use `[Maintenance] Backfill SourceForge Release Mirror` with an existing
published release tag to mirror its current GitHub assets and attach verified
`sourceforge-mirrors.json` metadata to that release. It does not rebuild the
release or publish a new app feed.

Repository variables are unset by default, so a fresh setup continues to publish
GitHub Releases and Surge without pretending that either optional mirror is
configured.

### Debug and recover without rebuilding

Configured storage is checked before nightly and release package builds. Run
`deno task storage:preflight` locally with the same environment to check
required configuration, archive authentication/database access, and SourceForge
SFTP connectivity. The Archive API receives an invalid manifest and a lookup for
an impossible build identity; no build or file is created. SFTP only lists the
project directory. This does not prove IA S3 upload permission, Worker IA
configuration, or SourceForge write permission.

Use `[Maintenance] Debug and Recover Storage Publication` after correcting a
configuration or deploying an uploader fix. For run `37233994404`, enter that
value as `source_run_id`. Start with `mode=preflight`; then choose
`mode=register`, `backend=archive`, and one target to reproduce archive
registration without a large transfer. The retained report includes the actual
HTTP error and retry count. Schema/configuration errors stop immediately;
network failures retain bounded retries. Registration creates immutable index
metadata, but uploads no package bytes. It is not an archive-completion check.

IA uploads require `curl` (available on the Ubuntu Actions runner). Each target,
registration, file, HTTP response, and retry is logged. Active transfers report
curl's progress every 30 seconds. Connections have a 15-second deadline;
transfer speeds below 1 KiB/s for 90 seconds abort, and each request has a
30-minute overall limit. Uploads attempt at most three times. Redirects remain
manual and restricted to IA S3 hosts; credentials are passed through stdin
rather than process arguments or temporary files. Recovery skips previously
indexed files only when their size and MD5 match the original publication.

Choose `mode=upload` and `backend=sourceforge`, `archive`, or `both` to
retransfer one target or all six using current scripts. Selecting `both` runs
the backends in separate jobs with independent locks and report artifacts.
Choose `mode=verify` to recheck public SourceForge hashes or reconcile IA
ingestion without reuploading bytes. Verification downloads SourceForge files to
calculate SHA-256, so its transfer cost is proportional to the selected target
inventory.

Recovery downloads finalized, signed packages and `publication-reports-central`
from the selected completed package run. It preserves the original run/attempt,
commit, item identifiers, and publication timestamp, and rejects any differing
file size/hash or metadata before writing remotely. It requires retained,
unexpired GitHub artifacts and original manifests. Missing manifests fail
closed; do not invent a new timestamp for an already registered build. A
recovery report is saved separately from the original run, which continues to
show its historical failure. SourceForge writers share a lock with normal
publication and backfills.

The workflow does not move tags, rebuild/sign packages, delete archives, or
publish updater feeds. After recovering all targets, its artifact contains a
verified `recovery-reports/sourceforge-mirrors.json`; use that complete metadata
with the existing updater workflow only when it belongs to the current release.
A partial target manifest must not replace the complete updater mirror
inventory.

For a locally retained manifest, registration alone is also available through
`deno task archive:register --manifest <manifest.json>
--server https://archive.nyanpasu.org --report <report.json>`.
It requires the archive upload token and item prefix, but does not require IA S3
credentials.

The related Deno tasks are `prepare:central-publication`,
`prepare:publication-manifest`, `sourceforge:upload`, `sourceforge:verify`,
`sourceforge:web-publish`, `sourceforge:cleanup`, `archive:publish`,
`archive:verify-reports` and `archive:verify`. `archive:publish` calls the local
IA uploader; `nyanpasu-file-list` only provides registration, verification,
indexing and download routes. `sourceforge:verify` writes the compact mirror
manifest consumed by the updater task, while `sourceforge:cleanup` accepts only
full run/attempt/SHA nightly directory names and never scans or deletes release
paths.

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
wait for GitHub release assets to finish uploading; notification jobs do not
download or upload packages. To resend a release notification after the updated
workflow is available on GitHub, manually run
`[Reusable] Notify Telegram of Releases` with `nightly: false` and the published
`tag`, for example `v2.0.0-beta.1`. This sends only the notification to
`@keikolog`; it does not rebuild packages. Re-running an older failed
publication run uses that run's original workflow and scripts.
