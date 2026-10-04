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
sleeps, frame-refresh calls, or corrective resizes. The default experiment's call
order is unchanged by the additional usage controls below.

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

### API usage controls

These distinguish the application-like call order from other uses of the same
public APIs. Run each separately, without the appearance flags above:

```powershell
cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --builder-size 2>&1 |
  Tee-Object -FilePath tao-5411-builder-size.log

cargo run --locked --manifest-path backend/repro-window-5411/Cargo.toml -- --defer-restore 2>&1 |
  Tee-Object -FilePath tao-5411-defer-restore.log
```

- **`--builder-size`**: supply the physical target directly to `with_inner_size`.
  Do not call `set_inner_size` at all. Keep the same minimum, position, hidden
  creation, shadow, transparency, and show/destroy behavior.
- **`--defer-restore`**: create the usual hidden `800x800` logical window, but
  process one event batch before calling `set_inner_size` and showing it. Record
  `created_after_events` before the setter, then measure after another batch.
  A user event wakes the measurement batch without changing window geometry.
- **`--visible`**, from the appearance controls: create an already-visible
  window, rather than restoring a hidden one. This tests the visibility condition,
  not just the event-loop timing.

`--builder-size` and `--defer-restore` select different sizing modes; do not combine
them. If both are supplied, the last sizing flag wins.

Interpret the results together:

| Comparison                             | Question                                                          |
| -------------------------------------- | ----------------------------------------------------------------- |
| Default vs. builder-size               | Does the error require the post-creation size setter?             |
| Default vs. defer-restore              | Is setting size before creation events are processed the trigger? |
| Default vs. visible                    | Is the error specific to restoring a hidden window?               |
| Default vs. decorated/no-shadow/opaque | Which appearance conditions are necessary?                        |

A passing control does not by itself prove the default call order is API misuse;
it only identifies a trigger. Tao 0.37.1 documents
[`set_inner_size`](https://docs.rs/tao/0.37.1/tao/window/struct.Window.html#method.set_inner_size)
as modifying the client size and
[`set_visible`](https://docs.rs/tao/0.37.1/tao/window/struct.Window.html#method.set_visible)
as changing visibility. These API docs do not state a Windows requirement to show
the window or process an event batch before setting its size. The builder accepts
both logical and physical sizes. At the reported 150% scale, the initial restore
height `943` is above the minimum `600 * 1.5 = 900`, so minimum-size clamping does
not explain the observed `+43` delta.

### Evidence

Snapshots include the requested, inner and outer physical sizes, their difference,
DPI scale, HWND, position, visibility, decorations, and shadow state. The `created`
snapshot compares against the initial builder size, not the restore target.
`restore_submitted` is an observation after the setter, not an event acknowledgement.
`before_destroy` measures the result after processing the event batch.

Each round prints `restore_mismatch` and `(width_delta, height_delta)`. The final
`SUMMARY` includes the mismatch count and initial/final client sizes:

- Exit **0**: all five final client sizes matched their targets.
- Exit **1**: at least one final size differed from its target; inspect the deltas.
  The `restore_mismatch`/`restore_mismatches` field names are retained in builder-size
  mode too, although that mode uses no size setter.
- Exit **2**: unsupported platform or unknown argument.

Send the baseline and control logs, particularly the `created`,
`created_after_events` (deferred mode), `before_restore`, `before_destroy`, and
`SUMMARY` lines. On the reported 150% scale, the application showed a non-client
height changing from `56` to `13` and restore height deltas of `+43`. Reproducing
that here isolates the behavior to a Tao-only call path without Tauri/Wry, but
API usage and timing must still be assessed. The controls are experiments, not
proposed application workarounds.

By default this program records Tao API results/events, **not** raw
`WM_NCCALCSIZE` messages. The reported controls gave zero final deltas in builder-size
and visible modes, but `+43` in all five deferred-restore cycles. Waiting for a
creation-event batch did not resolve the hidden-window layout discrepancy.

See [Native Tao traces](native-trace.md) for the next experiment: apply the included
**diagnostic-only** patch to a pinned, separate Tao checkout and run a copied probe
with local dependencies. It records native rectangle input/output and distinguishes
`wParam == FALSE` from missing-userdata fallbacks. Neither application dependencies
nor tracked lockfiles are changed. No upstream fix has been established yet.

## Compile checks from Linux

With the Rust Windows GNU target installed, the Windows-specific reproducer can
be typechecked without linking or running a Windows executable:

```sh
cargo check --locked --manifest-path backend/repro-window-5411/Cargo.toml --target x86_64-pc-windows-gnu
cargo clippy --locked --manifest-path backend/repro-window-5411/Cargo.toml --target x86_64-pc-windows-gnu --all-targets
cargo fmt --manifest-path backend/repro-window-5411/Cargo.toml -- --check
```

Runtime verification still requires Windows.
