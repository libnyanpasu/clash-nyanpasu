# nyanpasu-widget design

## Goal

A widget process that costs almost nothing while it sits on the desktop:

- steady-state working set under 30 MB;
- at most 4 threads created by the process itself (UI, IPC receiver and at
  most two more);
- icons and logo sharp at any scale factor;
- real per-pixel transparency: rounded corners and the translucent large card
  show the desktop, with no native frame, border or shadow.

The egui widget it replaces used about 150 MiB of working set, 240 MiB of
dedicated GPU memory and 32 threads, because wgpu loaded D3D12, Vulkan and
OpenGL drivers. Its icons were blurred: SVGs were rasterized at the logical
size and upscaled with linear filtering, and their premultiplied pixels were
premultiplied again.

## Layout

- `nyanpasu-widget-protocol` (separate crate): the render-free contract,
  `Message`, `StatisticMessage`, `Logo`, `LogoPreset` and
  `StatisticWidgetVariant`,
  and the channel over `nyanpasu-local-channel`: behind the `ipc` feature
  the widget's blocking client and receiver loop, behind `ipc-host` the
  application's Tokio server and sender. `nyanpasu-config` uses the types
  without either feature, so it depends on no IPC or GUI crate.
- `ui/theme.slint`: colours, sizes and fonts of both windows.
- `ui/state.slint`: the `StatisticData` global the application's data flows
  into, and the `LogoPreset` enum.
- `ui/components.slint`: `WidgetWindow`, `WidgetSurface` (drag to move),
  `LogoBadge`, `IconBadge`, `StatPill`, `CompactValue`.
- `ui/large.slint`, `ui/small.slint`: the two windows, composed from the
  components; `ui/widgets.slint` exports them.
- `src/lib.rs`: `start_statistic_widget`, which connects to the application,
  selects the backend and creates the window, in that order, so a widget with
  no application to talk to never opens a window.
- `src/view.rs`: `StatisticView`, implemented once for every window that
  uses `StatisticData`, so the IPC wiring does not depend on the variant.
- `src/app.rs`: the event loop, the IPC receiver thread and `Stop` handling:
  quit the event loop, and exit the process anyway if that has not happened
  within 5 s.
- `src/platform.rs`: backend and renderer selection, the Windows IME switch
  and window attributes.
- `src/logo.rs`: image logos, decoded from PNG and rasterized at the badge's
  physical size.
- `src/format.rs`: byte and speed formatting.
- `src/render_tests.rs`: the headless rendering test.

## Logos

The application sends `Message::UpdateLogo` with either the bundled cat in
one of its three states (`Logo::Builtin`) or a PNG (`Logo::Image`), and sends
it again after every start; the widget itself knows nothing of the settings
behind it.

- The bundled cat is drawn from `logo.svg`, `logo-tun.svg` and
  `logo-system-proxy.svg`, the tray icon's artwork in the tray's colour for
  each state, on the translucent disc. Being vectors, they are rasterized at
  the physical size like the other icons.
- An image fills the badge instead of the disc. Slint 1.18's software
  renderer scales an image by taking the nearest source pixel
  (`draw_texture_line`), which drops most of a 192 px artwork shrunk to 24
  to 88 px and leaves it jagged. So the IPC thread decodes the PNG into
  premultiplied RGBA once, and the event loop fits it into the badge's
  physical square with Lanczos3 (`fast_image_resize`, on premultiplied
  pixels so transparent pixels add no dark fringe), sharpens a minified
  result with a 3 × 3 unsharp mask (amount 0.35, colour clamped to alpha)
  and hands it to Slint at exactly that size,
  so it is drawn unscaled. The decoded source stays in `StatisticData` and is
  rasterized again on `ScaleFactorChanged`.
- An image that does not decode shows the bundled cat.

## Rendering and platform choices

| Platform | Renderer         | Transparency source                                                                                        |
| -------- | ---------------- | ---------------------------------------------------------------------------------------------------------- |
| Windows  | software         | softbuffer 0.4.8 `BitBlt` + winit's `DwmEnableBlurBehindWindow`; DWM composites the 32 bpp data with alpha |
| macOS    | software         | softbuffer 0.4.8 patched to `kCGImageAlphaPremultipliedFirst`; winit makes the `NSWindow` non-opaque       |
| Linux    | femtovg (OpenGL) | GL surface with alpha, X11 with a compositor (source reading, not run)                                     |

On Windows the process also:

- calls `ImmDisableIME(u32::MAX)` before the first window, which keeps the
  text services framework out of a window that takes no input (on a desktop
  where no text service host serves the process, IME support otherwise cost
  12 threads and about 17 MiB);
- creates the window with `with_drag_and_drop(false)`, since OLE drag and drop
  starts a COM worker thread;
- hides the window from the taskbar.

On macOS the event loop uses the `Accessory` activation policy, so the widget
has no Dock icon.

Slint's default Windows renderer (Skia on D3D12) was rejected: it draws a
black background instead of transparency and costs about 146 MiB.
femtovg on Windows costs about 71 MiB, of which the NVIDIA GL driver accounts
for about 45 MiB and two threads.

## Measured on Windows

Windows 11, 2 × 3840 × 2160 at 200 %, release builds, median of 5 runs after a
warm-up (large / small). The Slint column is the standalone
`nyanpasu-statistic-widget` binary:

| Metric                           | egui              | Slint            |
| -------------------------------- | ----------------- | ---------------- |
| Working set                      | 149.8 / 148.9 MiB | 20.3 / 20.0 MiB  |
| Private commit                   | 371 / 370 MiB     | 3.4 / 3.2 MiB    |
| Dedicated GPU memory             | 239 / 238 MiB     | 0                |
| Threads (created by the process) | 32 (3)            | 7 (3)            |
| Spawn to IPC handshake           | 629 / 618 ms      | 27 / 27 ms       |
| CPU at 1 update/s                | 4.75 / 4.18 ms/s  | 1.12 / 0.88 ms/s |
| Stop to exit                     | 144 / 143 ms      | 16 / 21 ms       |

The three threads the Slint process creates are the UI thread, the IPC
receiver and softbuffer's DC allocator. The other four (two UI Automation
threads, a COM RPC worker and thread-pool workers) are loaded by another
process's desktop-wide WinEvent hook; they appear in every GUI process on
that desktop and disappear on a separate desktop. Process mitigation policies
set in `main` (extension points off, Microsoft-signed images only, no remote
or low-label images) neither removed them nor blocked anything, since every
loaded DLL was already Microsoft-signed.

The application starts the widget as a subcommand of its own 57 MiB
executable, which costs more than the standalone binary (median of 3 runs
after a warm-up, large / small):

| Metric                           | Application subcommand |
| -------------------------------- | ---------------------- |
| Working set                      | 23.8 / 23.5 MiB        |
| Private working set              | 3.1 / 2.8 MiB          |
| Private commit                   | 4.2 / 3.9 MiB          |
| Threads (created by the process) | 7 (3)                  |
| Loaded modules                   | 44 (standalone: 36)    |
| Spawn to IPC handshake           | 385 / 393 ms           |
| Stop to exit                     | 16 / 22 ms             |

The application imports ten more Microsoft system DLLs (`crypt32`, `wininet`,
`ws2_32` and the like) and, linking the C runtime statically, two fewer. Most of the extra startup time
passes before the subcommand is parsed: from the same build directory, the
application takes about 400 ms to reject an invalid variant, the standalone
binary about 110 ms. The application used to build its async runtime, one
worker thread per CPU, before parsing the subcommand; it now builds it only
for the GUI. That removed 32 threads from the widget process and about 30 ms
from its startup.

## Measured on macOS

Apple M3, macOS 27, built-in display at 2×, release builds of the standalone
widget driven by a fake application over the widget channel (large / small).
Memory is `phys_footprint`, the figure Activity Monitor shows; power is the
process's `ri_energy_nj` delta. 1 update/s is the median of 3 runs, the other
rows are single runs:

| Metric                        | femtovg (OpenGL) | software (patched softbuffer) |
| ----------------------------- | ---------------- | ----------------------------- |
| Footprint at 1 update/s       | 77.9 / 76.7 MiB  | 16.3 / 16.1 MiB               |
| of which graphics (GL driver) | 54.4 / 54.4 MiB  | 0.02 / 0.02 MiB               |
| CPU at 1 update/s             | 4.1 / 4.1 ms/s   | 4.6 / 2.7 ms/s                |
| Power at 1 update/s           | 0.67 / 0.49 mW   | 0.98 / 0.32 mW                |
| CPU at 10 updates/s           | 41.6 / 33.0 ms/s | 42.6 / 24.6 ms/s              |
| Footprint after 30 s idle     | 24.8 / 23.4 MiB  | 17.4 / 16.2 MiB               |
| Spawn to IPC handshake        | about 8 ms       | about 9 ms                    |

The GL driver gives its memory back only after about 30 s without a frame, so
a widget updated every second keeps it. `screencapture -l` of both renderers
shows the same transparent corners (alpha 0) and translucent large card (alpha
127). Without the patch the software renderer draws them opaque black.

softbuffer 0.4.8's CoreGraphics backend creates its image with
`kCGImageAlphaNoneSkipFirst`, which ignores the alpha byte. The workspace
patches softbuffer with
[libnyanpasu/softbuffer@`nyanpasu/v0.4.8-cg-premultiplied-alpha`](https://github.com/libnyanpasu/softbuffer/tree/nyanpasu/v0.4.8-cg-premultiplied-alpha),
which uses `kCGImageAlphaPremultipliedFirst` instead. Slint's software
renderer writes premultiplied ARGB and never a colour channel above its alpha,
so CoreGraphics clamping such pixels (softbuffer issue #356) does not apply.
Tauri only uses softbuffer on Windows, so the patch reaches nothing else.
softbuffer's master branch has an `AlphaMode` API (PR #321), but it is
unreleased and changes the 0.4 API that Slint 1.18.1 uses.

The software renderer's colours are colour-managed, femtovg's are not. On a
Display P3 screen the small widget's `#eaddff` captures as (232, 221, 252) in
software, which is `#eaddff` converted from sRGB to Display P3, and as
(234, 221, 255) in femtovg. softbuffer tags its image with device RGB, which
macOS treats as sRGB, so Core Animation converts it to the display's colour
space; the GL surface carries no colour space, so its sRGB values pass through
as display values and look slightly more saturated, as on Windows, which does
not colour-manage either. On an sRGB display both are identical.

Still to check on macOS:

- no flicker while dragging or updating;
- correct scale when moving between displays with different
  `contentsScale`;
- no window shadow.

## Open work: Linux

Not implemented yet. The femtovg path loads a GL driver (about 70 MiB on
Windows with NVIDIA, about 54 MiB on macOS; Linux has not been measured), and
softbuffer 0.4.8 has no per-pixel alpha on Linux.

### Linux

- **X11.** winit picks a depth-32 TrueColor visual for a transparent window
  and softbuffer uploads with the window's depth, so the software renderer
  probably already shows real alpha under a compositor (source reading, not
  tested). Without a compositor the corners turn black; detect that with the
  `_NET_WM_CM_S<screen>` selection owner and fall back to an opaque
  background.
- **Wayland.** softbuffer 0.4.8 hard-codes `Xrgb8888` (a two-line change to
  `Argb8888` in a fork). The bigger problem is that an `xdg_toplevel` can
  neither position itself nor stay on top, which the widget needs, and the
  egui widget probably had the same problem. The likely fix is to run the
  widget on X11/XWayland (`with_x11()` on the winit event loop builder).
- **Plan:** software renderer on X11 with the compositor check, forced
  X11/XWayland on Wayland sessions, femtovg kept as a fallback.
- **Build note:** Linux builds need the fontconfig development package
  (fontique links it); `RUST_FONTCONFIG_DLOPEN=on` does not work with
  fontique. A cross check from Windows can skip the build script with
  `--config "target.x86_64-unknown-linux-gnu.fontconfig.rustc-link-lib=['fontconfig']"`.

## Upstream

Report premultiplied alpha for the Win32 and X11 backends in softbuffer and
help Slint move to softbuffer 0.5; drop the fork once that ships.

## Other deferred work

- **Two threads instead of three.** A custom Slint platform presenting with
  `UpdateLayeredWindow` (and the native equivalents elsewhere) would drop
  softbuffer's DC allocator thread and give per-pixel alpha on every platform,
  but re-implements winit's window adapter (estimated 1.5–3k lines).
- **A standalone widget binary.** Shipping `nyanpasu-statistic-widget` next to
  the application would save the subcommand's extra 3.5 MiB of working set and
  about 290 ms of startup, at the cost of a second executable to package and
  sign.
- **Mitigations at spawn time.** Policies set in `main` only affect DLLs
  loaded afterwards. Blocking DLLs injected while the process is created
  needs the parent to pass `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` when it
  spawns the widget; worth revisiting only if third-party hook DLLs show up in
  the widget process.
