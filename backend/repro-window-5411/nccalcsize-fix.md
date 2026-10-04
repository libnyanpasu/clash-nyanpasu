# FALSE-form NCCALCSIZE fix candidate for #5411

**Status:** the native logs establish the failure mechanism. The included Tao fix
candidate typechecks, but still requires Windows runtime verification. It is not
wired into the Nyanpasu application, and no upstream PR has been submitted.

## Confirmed native sequence

Both supplied native logs load the instrumented local Tao 0.37.1. Default mode
still grows by `43` in every cycle; builder-size mode finishes with zero deltas.
The first default cycle records:

1. `CreateWindowExW` receives outer size `1222x1213` for requested client size
   `1200x1200`, accounting for custom shadow insets `(left=11, top=2, right=11,
bottom=11)`.
2. During `WM_NCCREATE`, an early frame refresh sends TRUE-form `WM_NCCALCSIZE`
   before userdata exists. Default processing produces client RECT
   `(11,45,1211,1202)`, hence client size `1200x1157`.
3. **After userdata installation**, Windows sends FALSE-form `WM_NCCALCSIZE`.
   Tao knows `decorated=false`, `shadow=true`, `fullscreen=false`, but explicitly
   chooses `route_default_false`. The returned RECT is again
   `(11,45,1211,1202)`. This is the last recorded calculation before `WM_CREATE`
   and the hidden window returning from construction. Its final non-client height
   remains `45+11=56`.
4. The client-size setter requests `1266x943`, measures offset `(22,56)`, and
   submits `1288x999` to its native sizing helper.
5. This resize sends TRUE-form `WM_NCCALCSIZE` **with userdata available**. Tao
   applies custom insets `(11,2,11,11)`. The resulting client height is
   `999-(2+11)=986`, not `943`.

Therefore the observed excess is exactly:

```text
old non-client height - new non-client height
= (45 + 11) - (2 + 11)
= 43 physical pixels
```

The other four default cycles repeat the same sequence. Width does not grow
because left/right insets stay at `11+11` in both calculations. In builder-size
mode, outer height starts at `956`; showing the window switches the client height
from `900` to `943` without resizing that outer frame.

The early missing-userdata calculation is real, but moving userdata installation
alone would not fix the final FALSE-form default calculation. Waiting for events
also cannot help: the deferred control already reproduced the same error. The
application's hidden-window resize is a trigger, not an identified API violation.

The native lines use stderr and probe snapshots use stdout; PowerShell can merge
them slightly out of order. The conclusion uses the native message sequence,
returned RECTs, subsequent state, and source call structure, not a presumed total
ordering between the two output streams.

## Fix scope

[`tao-0.37.1-nccalcsize-fix.patch`](tao-0.37.1-nccalcsize-fix.patch) applies to
**pristine** tag `tao-v0.37.1`, revision
`37b7e8bc90a050e93be988df636f322c3ef147b6`. It changes only the Windows
`WM_NCCALCSIZE` handler:

- Decorated windows retain `DefWindowProcW` handling.
- Undecorated windows select the new client RECT from either the FALSE-form `RECT`
  or the TRUE-form `NCCALCSIZE_PARAMS.rgrc[0]`.
- Both forms use the existing maximized work-area, taskbar-edge, shadow inset,
  and fullscreen conditions. The TRUE-form old rectangles are not modified.
- FALSE custom handling returns zero, as required by the
  [Win32 contract](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize).

There are no constant pixel corrections, extra resizes, sleeps, frame-refresh
calls, or changes to userdata installation. There are no application call-order,
dependency, or persistence changes. The patch contains no diagnostic logging.

## Replace diagnostics with the candidate on Windows

Update `fix/window-height-5411` first. The paths below reuse the checkout and
copied probe from [native trace preparation](native-trace.md). Apply this **once**;
if any check fails, inspect the checkout rather than resetting local changes.

```powershell
$base = Join-Path (Get-Location).Path 'backend/repro-window-5411'
$native = Join-Path $base 'target/native-trace'
$tao = Join-Path $native 'tao'
$probe = Join-Path $native 'repro'
$manifest = Join-Path $probe 'Cargo.toml'
$target = Join-Path $base 'target'
$tracePatch = Join-Path $base 'tao-0.37.1-native-trace.patch'
$fixPatch = Join-Path $base 'tao-0.37.1-nccalcsize-fix.patch'

$revision = git -C $tao rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne '37b7e8bc90a050e93be988df636f322c3ef147b6') {
  throw 'Unexpected Tao revision; do not continue'
}

# Remove only the known diagnostic changes; preserve the existing copied probe.
git -C $tao apply --reverse --check $tracePatch
if ($LASTEXITCODE -ne 0) { throw 'Cannot safely remove diagnostics; do not continue' }
git -C $tao apply --reverse $tracePatch
if ($LASTEXITCODE -ne 0) { throw 'Diagnostic removal failed; do not continue' }

git -C $tao apply --check $fixPatch
if ($LASTEXITCODE -ne 0) { throw 'Fix check failed; do not continue' }
git -C $tao apply $fixPatch
if ($LASTEXITCODE -ne 0) { throw 'Fix application failed; do not continue' }

git -C $tao diff --stat
cargo tree --locked --manifest-path $manifest -i tao
if ($LASTEXITCODE -ne 0) { throw 'Dependency inspection failed' }
```

The diff should contain only `src/platform_impl/windows/event_loop.rs`; the tree
should still point to the local Tao checkout. The local dependency paths did not
change, so no lockfile regeneration is needed. Keep the original native logs as
before-fix evidence. Do not run trace setup again or overwrite the patched copied
manifest.

## Regression runs

At the reported 150% scale, run all seven modes in the same PowerShell session:

```powershell
foreach ($mode in @('default', 'builder-size', 'defer-restore', 'visible', 'decorated', 'no-shadow', 'opaque')) {
  $options = @()
  if ($mode -ne 'default') { $options = @("--$mode") }
  $log = "tao-5411-fixed-$mode.log"
  cargo run --locked --manifest-path $manifest --target-dir $target -- @options 2>&1 |
    Tee-Object -FilePath $log
  $result = $LASTEXITCODE
  Write-Host "mode=$mode exit=$result"
}
```

Send all seven logs, including any failure. Each mode still runs five cycles with
unchanged probe logic. Expected results on this system:

- Default and deferred modes: `created` already has client size `1200x1200` while
  hidden. Restoring to `1266x943` gives exactly that client size before showing.
- Builder-size mode: `created` already has client size `1266x943` while hidden;
  first show no longer changes its client size.
- All modes: every `restore_mismatch=false`, every delta `(0,0)`,
  `restore_mismatches=0`, final size `1266x943`, and exit **0**.
- No `[tao-native-5411]` lines: diagnostics were deliberately removed. Dependency
  tree inspection, the applied diff, and Cargo compilation establish which source
  is used.

The minimum may clamp requests at other DPI scales, as explained in the baseline
README. These checks cover this report's normal window state; other DPI values,
multi-monitor transitions, maximized/fullscreen behavior, and Windows 10 still
need native regression coverage before treating the candidate as release-ready.

## Checks performed on Linux

- Diagnostic patch reversal and candidate application on an independent clean
  checkout of the pinned tag.
- Windows GNU target Cargo check and Clippy for the probe and Tao library.
- Rustfmt for the changed upstream file, Markdown formatting, and patch whitespace.

These are compile/static checks, **not** proof that the Windows behavior is fixed.
