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
Releases, or GHFast. GHFast is optional and does not change saved source order.
Its URLs prefix the complete GitHub URL with `https://ghfast.top/`; Nyanpasu
uses the GitHub path without its host. Every package source retains the same
release signature and updater public key.

Surge's [deployment API](https://surge.sh/docs/api/deploys) documents a 450 MB
project limit. Hosting only the archives referenced by a manifest avoids keeping
historical releases, but does not guarantee that a complete current release
fits. For example, on 2026-10-02 the nightly feed referenced seven distinct
archives totalling about 1.285 GB. Mirroring all of them would require separate
sites or another storage provider. The existing manifest site cannot host that
complete set of archives.

## Archive uploads and Telegram notifications

Package build jobs upload installers and portable bundles to
`archive.nyanpasu.org` using `nightly/<short commit hash>` or
`release/<release tag>` as `FOLDER_PATH`. Both publication workflows verify all
platform upload reports before sending a Telegram notification. Failed uploads
retain their HTTP error details in the job log and `upload-diagnostics-*`
artifacts.

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
succeeded. Re-running only `Verify Archive Uploads` rechecks the same reports;
it does not upload missing files. After capacity is restored, retry the affected
upload from the retained artifacts into its original `FOLDER_PATH`, or start a
fresh nightly build. Avoid rebuilding all platforms merely to diagnose a storage
quota error.

Telegram notifications contain release/build information and the archive home
link; notification jobs do not download or upload packages. To resend a release
notification after the updated workflow is available on GitHub, manually run
`[Reusable] Notify Telegram of Releases` with `nightly: false` and the published
`tag`, for example `v2.0.0-beta.1`. This sends only the notification to
`@keikolog`; it does not rebuild packages or repair missing archive uploads.
Re-running an older failed publication run uses that run's original workflow and
scripts.
