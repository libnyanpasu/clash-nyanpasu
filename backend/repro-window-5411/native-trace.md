# Native Tao traces for #5411

This is a **diagnostic experiment, not a fix**. The patch adds stderr output to a
separate Tao checkout. It does not add frame-refresh calls, alter message routing,
change rectangles, or move userdata installation. Existing Tao frame-refresh calls
are logged, not introduced by the patch.

## Evidence so far

The reported Windows 11 system uses 150% scaling. Five cycles in each mode gave:

| Mode             | Final size delta per cycle | Observation                                                          |
| ---------------- | -------------------------- | -------------------------------------------------------------------- |
| Default          | `(0, +43)`                 | Hidden creation, immediate size setter                               |
| Builder size     | `(0, 0)`                   | Showing changes client height `900 -> 943`, outer height stays `956` |
| Deferred restore | `(0, +43)`                 | Processing creation events leaves the non-client height at `56`      |
| Visible creation | `(0, 0)`                   | Non-client height is already `13` before the setter                  |

The deferred result rules out merely waiting for the creation-event batch. The
builder and visible results isolate first-show layout from the size setter. They
do not establish an API requirement that a window must be shown before resizing.

Two source paths need to be distinguished:

1. `InitData::create_window` applies initial flags during `WM_NCCREATE`, before
   `GWL_USERDATA` is installed. `WindowFlags::apply_diff` can synchronously refresh
   the frame; reentrant messages with no userdata go to `DefWindowProcW`.
2. After userdata is installed, `WM_NCCALCSIZE` with `wParam == FALSE` still goes to
   `DefWindowProcW`, even for an undecorated window. The TRUE form uses custom
   insets. FALSE supplies a `RECT`; TRUE supplies `NCCALCSIZE_PARAMS`.

Microsoft's [WM_NCCALCSIZE contract](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize)
requires the FALSE-form rectangle to be converted from the proposed window area to
the client area too. FALSE changes the valid-pixel preservation contract, not the
need to calculate client geometry.

Initial style retains `WS_CAPTION` for this top-level window. The default and
custom calculations can therefore disagree. Native traces must identify which
calculation last establishes the client rectangle before the builder returns.
Do not assume that seeing an early no-userdata fallback alone proves the cause;
a later calculation may overwrite its result.

## Prepare on Windows

Run **once**, from the Nyanpasu repository root, after updating the investigation
branch. Git, Cargo, and the normal Windows Rust toolchain are required. Everything
created below lives in the reproducer's gitignored `target/native-trace` directory.
A copied manifest/lockfile avoids changing either tracked workspace or lockfile.
Only Tao and its Android workspace macro package switch to local sources; other
locked dependency versions are retained.

```powershell
$base = Join-Path (Get-Location).Path 'backend/repro-window-5411'
$native = Join-Path $base 'target/native-trace'
$tao = Join-Path $native 'tao'
$probe = Join-Path $native 'repro'
$patch = Join-Path $base 'tao-0.37.1-native-trace.patch'

New-Item -ItemType Directory -Force (Join-Path $probe 'src') | Out-Null

git clone --depth 1 --single-branch --branch tao-v0.37.1 https://github.com/tauri-apps/tao.git $tao
if ($LASTEXITCODE -ne 0) { throw 'Tao clone failed; do not continue' }

$revision = git -C $tao rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne '37b7e8bc90a050e93be988df636f322c3ef147b6') {
  throw 'Unexpected Tao revision; do not continue'
}

git -C $tao apply --check $patch
if ($LASTEXITCODE -ne 0) { throw 'Patch check failed; do not continue' }
git -C $tao apply $patch
if ($LASTEXITCODE -ne 0) { throw 'Patch application failed; do not continue' }

Copy-Item (Join-Path $base 'Cargo.toml'), (Join-Path $base 'Cargo.lock') -Destination $probe
Copy-Item (Join-Path $base 'src/main.rs') -Destination (Join-Path $probe 'src/main.rs')

$manifest = Join-Path $probe 'Cargo.toml'
$target = Join-Path $base 'target'
$taoToml = $tao.Replace('\', '/') | ConvertTo-Json -Compress
[System.IO.File]::AppendAllText($manifest, "`n[patch.crates-io]`ntao = { path = $taoToml }`n")

# The first check updates only the copied lockfile to use local Tao sources.
cargo check --manifest-path $manifest --target-dir $target
if ($LASTEXITCODE -ne 0) { throw 'Native probe compilation failed' }
cargo tree --locked --manifest-path $manifest -i tao
if ($LASTEXITCODE -ne 0) { throw 'Dependency inspection failed' }
```

The tree must show `tao v0.37.1 (.../target/native-trace/tao)` under the reproducer,
not registry Tao. The tag's Windows source files were compared with the published
0.37.1 crate before making this patch. The published crate's VCS metadata names the
same revision. No application dependency is patched.

If setup was already completed, do not clone or apply the patch again. To resume
in a new PowerShell session, redefine `$base`, `$native`, `$probe`, `$manifest`, and
`$target` using the paths above. Do not copy the original manifest over the patched
copy or append the patch section twice.

## Capture

Use the same PowerShell session as preparation:

```powershell
cargo run --locked --manifest-path $manifest --target-dir $target 2>&1 |
  Tee-Object -FilePath tao-5411-native-default.log

cargo run --locked --manifest-path $manifest --target-dir $target -- --builder-size 2>&1 |
  Tee-Object -FilePath tao-5411-native-builder-size.log
```

Send both complete logs. Default mode is expected to exit **1** if the original
mismatch still occurs; this is the reproducer's assertion, not a crash. Builder
mode is expected to exit **0** on the reported system. If instrumentation changes
these outcomes, report that too rather than treating it as a fix.

New lines have prefix **`[tao-native-5411]`**. Without that prefix, the instrumented
Tao was not loaded. Each HWND correlates with the usual `round` snapshots. The
native `HWND` Debug representation may be hexadecimal; the probe prints its raw
integer. Native message codes `0x81` and `0x1` are `WM_NCCREATE` and `WM_CREATE`.

Important markers:

- `create_window_before_initial_flags` / `create_window_after_initial_flags` and
  `WM_NCCREATE_userdata_installed`: bracket early frame changes and installation.
- `message=WM_NCCALCSIZE stage=entry`: records the incoming rectangle, `wparam`,
  and userdata. `rect` is the FALSE-form RECT or TRUE-form `rgrc[0]`, respectively.
- `exit_default_no_userdata`, `route_default_false`, `route_default_decorated`,
  `exit_default`, `exit_custom`: distinguish fallback reasons and returned rects.
- `nccalc_route`: records existing recursion depth, flags, decorations, shadow,
  and fullscreen without acquiring additional actor/window-state locks.
- `nccalc_shadow_insets`: records the custom inset calculation.
- `set_inner_size_offsets` / `set_inner_size_native_input`: show exactly which
  measured border compensation was added to the requested client dimensions.
- `apply_diff_before_frame_changed` / `apply_diff_after_frame_changed` and
  `set_visible_before` / `set_visible_after`: expose first-show layout changes.

Geometry snapshots include current client/outer RECTs, native styles, and userdata.
Read them together with the incoming/outgoing NCCALCSIZE rects: during a callback,
`GetClientRect` can still describe the previous layout, not the proposed one.
Creation-time queries may fail and are logged as `None`, not treated as errors.
The patch only copies message rectangles and uses read-only window queries. Log
write failures are ignored so a closed pipe does not panic across a Win32 callback.

## Verification limits

The patch applies to the pinned tag and passes Windows GNU target typechecking.
Native runtime verification requires Windows. The hypotheses above remain
hypotheses until the native message sequence is captured. A future fix must
preserve both RECT forms and decorated, maximized, fullscreen, and DPI semantics;
this diagnostic patch deliberately changes none of them.
