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

Telegram notifications contain release/build information and the archive home
link; notification jobs do not download or upload packages. To resend a release
notification after the updated workflow is available on GitHub, manually run
`[Reusable] Notify Telegram of Releases` with `nightly: false` and the published
`tag`, for example `v2.0.0-beta.1`. This sends only the notification to
`@keikolog`; it does not rebuild packages or repair missing archive uploads.
Re-running an older failed publication run uses that run's original workflow and
scripts.
