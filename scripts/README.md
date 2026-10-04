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
`SOURCEFORGE_USERNAME` variable and the `SOURCEFORGE_SSH_KEY` and
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
