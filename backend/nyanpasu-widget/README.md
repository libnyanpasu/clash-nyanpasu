# nyanpasu-widget

The network statistic widget: a small frameless, always-on-top window that
shows the traffic totals and speeds the application sends it. It runs in its
own process so the main window's webview and GPU state never load into it,
and it is written in [Slint](https://slint.dev) with the software renderer on
Windows and macOS to stay under 30 MB of working set.

Two windows share one set of components:

| Variant | Logical size | Content                                                   |
| ------- | ------------ | --------------------------------------------------------- |
| `large` | 206 × 60     | logo, download/upload totals and speeds, translucent card |
| `small` | 80 × 32      | logo, upload/download speeds, follows the system theme    |

## Running

The application starts the widget as `clash-nyanpasu statistic-widget
<large|small>`; `nyanpasu-statistic-widget <large|small>` runs the same code
standalone. Either way the process connects back to the application before it
opens a window:

1. the parent creates a `nyanpasu-local-channel` one-shot server and passes
   its bootstrap in `NYANPASU_LOCAL_CHANNEL` (`Bootstrap::ENV`);
2. the widget connects with the blocking client on its receiver thread, so
   the widget binary links no async runtime;
3. the parent sends `Message::UpdateStatistic` / `UpdateLogo` and finally
   `Message::Stop`. A closed channel counts as `Stop`.

`nyanpasu_widget_protocol::ipc` wraps both ends: the `ipc` feature is the
widget's, `ipc-host` adds the application's Tokio server.

Without the variable, or without a server to connect to, the widget never
opens a window: the standalone binary returns the error (exit code 1), and the
application's subcommand panics with it (exit code 101).

The renderer is `software` on Windows and macOS and `femtovg` (OpenGL) on
Linux. `SLINT_BACKEND` overrides it, e.g. `SLINT_BACKEND=winit-software` on
Linux where no GL driver is usable.

## Tests

`cargo test -p nyanpasu-widget` covers the value formatting and renders both
windows headless at scale 1, 1.5 and 2. The render test checks the transparent
corners and the translucent card, compares the bundled logo in all three
states pixel by pixel with its SVG rasterized by resvg, checks that an image
logo is drawn exactly as `logo::rasterize` produced it, and checks that the
widest values
("888.88 MB", "888.88 MB/s") fit their cells while one more character does
not.

See [DESIGN.md](DESIGN.md) for the resource budget, the renderer choice per
platform, the macOS measurements and the open Linux work.
