# Tray icon presets

Windows settings offer two presets through the existing custom-icon upload RPC.
Applying a preset replaces the saved normal, TUN, and system-proxy icons; each
state can still be customized afterwards. Reset restores the original cat icon.
The three uploads are sequential: a failed upload may leave earlier states changed;
the settings refresh the saved icons even after failure.

| Preset      | Normal              | TUN              | System proxy              |
| ----------- | ------------------- | ---------------- | ------------------------- |
| Minimal cat | `cat/normal.png`    | `cat/tun.png`    | `cat/system-proxy.png`    |
| Mascot      | `mascot/normal.png` | `mascot/tun.png` | `mascot/system-proxy.png` |

The mascot assets are 192 × 192 RGBA PNGs, resized from `new-tray/win-normal.png`,
`new-tray/win-tun.png`, and `new-tray/win-sys.png` with macOS `sips -z 192 192`.
They preserve the source artwork and fit the existing 1 MiB upload limit. The
backend generates the DPI-scaled tray cache with its existing Lanczos3 resizer.

macOS continues to use `macos/template.png` as a template image. The entire tray
icon customization UI is hidden there: passing colored round mascot artwork through template
rendering would lose its internal color detail. A future macOS mascot option
needs a separately designed monochrome glyph or explicit non-template rendering;
it must also handle startup and subsequent tray refreshes consistently.

`cat/normal.ico` preserves the legacy Windows ICO asset; the current tray loader
uses the PNG variants. Application and installer icons remain at `icons/`.
