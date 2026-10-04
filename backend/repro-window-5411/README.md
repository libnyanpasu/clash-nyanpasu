# Tao-only reproducer for Nyanpasu #5411

This standalone Cargo project uses **Tao 0.37.1 only**. It does not use Tauri,
Wry, WebView2, tray icons, application configuration, or application actors.
It does not modify user configuration. Its own workspace and lockfile keep it
independent of the application build.

## Windows

Run from the repository root in PowerShell:

```powershell
cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml 2>&1 |
  Tee-Object -FilePath tao-5411-default.log
```

The program automatically runs five create/restore/show/destroy cycles. Windows
may briefly flash on screen; no clicking is required. Do not drag or resize them.
The first restore target, `1266x943` physical pixels, matches the reported Windows
log. Subsequent targets are the preceding cycle's actual client size.

Each cycle creates an `800x800` logical window at `(0,0)`, with a `400x600` logical
minimum, decorations disabled, shadow enabled, transparency enabled, and initial
visibility disabled. It moves the window to `(100,100)`, restores the target
client size, then shows it. After the event loop has processed the current batch
of events (`MainEventsCleared`), it captures the client size and drops the window.
The next cycle starts only after the old window's `Destroyed` event. There are no
sleeps, frame-refresh calls, or corrective resizes.

The fixed minimum can legitimately clamp a requested size on unusually high DPI
settings. Interpret mismatches together with the recorded scale factor and
minimum, rather than treating every mismatch as the issue.

### Controls

Change one condition at a time:

```powershell
cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --decorated 2>&1 |
  Tee-Object -FilePath tao-5411-decorated.log

cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --no-shadow 2>&1 |
  Tee-Object -FilePath tao-5411-no-shadow.log

cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --opaque 2>&1 |
  Tee-Object -FilePath tao-5411-opaque.log

cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --visible 2>&1 |
  Tee-Object -FilePath tao-5411-visible.log
```

`--no-shadow` is meaningful for undecorated windows; Windows still draws shadows
for decorated windows. Flags can be combined, but the commands above isolate one
variable each.

### Evidence

Snapshots include the requested, inner and outer physical sizes, their difference,
DPI scale, HWND, position, visibility, decorations, and shadow state. The `created`
snapshot compares against the initial builder size, not the restore target.
`restore_submitted` is an observation after the setter, not an event acknowledgement.
`before_destroy` measures the result after processing the event batch.

Each round prints `restore_mismatch` and `(width_delta, height_delta)`. The final
`SUMMARY` includes the mismatch count and initial/final client sizes:

- Exit **0**: all five final client sizes matched their restore targets.
- Exit **1**: at least one restore differed from its target; inspect the deltas.
- Exit **2**: unsupported platform or unknown argument.

Send all five logs, particularly the `created`, `before_restore`, `before_destroy`,
and `SUMMARY` lines. On the reported 150% scale, the application showed a non-client
height changing from `56` to `13` and restore height deltas of `+43`. Reproducing
that here would isolate the behavior to Tao without Tauri/Wry. The controls are
experiments, not proposed application workarounds.

This program records Tao API results/events, **not** raw `WM_NCCALCSIZE` messages.
A Tao-only reproduction must be followed by instrumentation of Tao's window
procedure to verify the precise native message sequence; these snapshots alone
cannot distinguish the `wParam == FALSE` and missing-userdata paths.

## Compile checks from Linux

With the Rust Windows GNU target installed, the Windows-specific reproducer can
be typechecked without linking or running a Windows executable:

```sh
cargo check --locked --manifest-path backend/repro-window-5411/Cargo.toml --target x86_64-pc-windows-gnu
cargo clippy --locked --manifest-path backend/repro-window-5411/Cargo.toml --target x86_64-pc-windows-gnu --all-targets
cargo fmt --manifest-path backend/repro-window-5411/Cargo.toml -- --check
```

Runtime verification still requires Windows.
