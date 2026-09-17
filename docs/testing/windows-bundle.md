# Windows bundle reuse (#5293)

For each architecture and release channel, compile the application once with
`tauri build --no-bundle`. The `standard`, `fixed-webview`, and `both` workflow
inputs select subsequent `tauri bundle` calls. Fixed packaging uses the static
`overrides/fixed-webview2.conf.json` override after the release/nightly config.

`prepare:release` and `prepare:nightly` remain shared across platforms. Windows
bundle selection adds no commands to `package.json` and no generated package
marker or descriptor. Installed and portable fixed packages both place their
runtime in `WebView2/` next to the application EXE.

## Startup selection

Before constructing the updater plugin or calling `Builder::build`, `AppStartup`
reads an injected `StartupSource` and returns an immutable `StartupSelection`.
The filesystem adapter inspects the executable directory:

- No `WebView2/`: keep the configured system runtime and default updater target.
- `WebView2/msedgewebview2.exe` exists: use the bundled runtime and the matching
  fixed updater target.
- `WebView2/` exists but is incomplete: report a startup error.

The selection policy has no Tauri or filesystem dependency. The Tauri adapter
applies the runtime choice to the context and the target to the updater plugin
builder. It never rewrites updater endpoints. Non-Windows startup retains its
existing configuration without inspecting Windows directories. The later
`setup::setup` phase continues to assemble `NyanpasuClient` and its actors.

Directory contents are authoritative. Extracting a standard portable package
onto an existing fixed package leaves it in fixed mode while `WebView2/` remains;
use a clean directory or remove that runtime to switch to standard mode.

## Shared updater manifests

Release clients share the existing `update.json` feed and its mirrors; nightly
clients share `update-nightly.json` and its mirrors. Each feed contains all
platforms and Windows variants. Proxy copies contain the same target set with
proxied artifact download URLs. There are no separately generated fixed feeds.

| Package                 | Manifest target                 |
| ----------------------- | ------------------------------- |
| Windows x86_64 standard | `windows-x86_64`                |
| Windows ARM64 standard  | `windows-aarch64`               |
| Windows x86_64 fixed    | `windows-x86_64-fixed-webview`  |
| Windows ARM64 fixed     | `windows-aarch64-fixed-webview` |

Linux/macOS targets and existing standard aliases remain in the same manifest.
Each archive is paired with its own signature; missing signatures or ambiguous
target assignments fail manifest generation. The updater workflow runs the generator
once per channel.

Older fixed clients have their separate feed URLs embedded in the executable.
They must install/extract this version manually before using the shared feed and
custom target. This change does not publish new updates to those old fixed feeds.

## Automated checks

```sh
deno test --config scripts/deno.jsonc -A scripts/windows-bundle_test.ts scripts/updater-platforms_test.ts
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib startup:: --locked
```

Rust tests cover directory detection, incomplete runtimes, source failures,
unchanged endpoints for both channels, native startup, and Tauri's selection of
standard/fixed artifacts from one manifest. Deno tests cover portable ZIP
contents, platform-neutral release preparation, shared packaging endpoints,
all manifest targets, and archive/signature pairing.

## Windows artifact acceptance

These checks require Windows and produced artifacts; unit tests do not establish
installer or WebView2 runtime acceptance.

1. Exercise both architectures, both channels, all three bundle inputs, and
   portable on/off. Confirm one application/frontend compilation per job, expected
   artifact names, and valid updater signatures.
2. Inspect installed NSIS and extracted portable layouts. Only fixed packages
   should include `WebView2/`; compare application payload hashes between variants
   from the same compilation.
3. Start fixed packages without system WebView2 and inspect the runtime process
   executable path. Standard packages must retain system-runtime/bootstrapper
   behavior. Test an incomplete runtime directory as well.
4. Observe that standard/fixed and Linux/macOS clients request the same feed for
   their channel. Verify each Windows target selects its own signed installer,
   then update and restart to confirm directory detection retains the variant.
5. Test fresh directories, covering an existing portable directory, NSIS upgrades,
   and uninstall resource cleanup. Migrate older fixed clients manually as noted
   above before testing subsequent automatic upgrades.

The workflow retains the Cargo build-directory workaround required by the pinned
`tauri-build`.
