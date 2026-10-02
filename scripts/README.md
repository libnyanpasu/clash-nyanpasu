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
