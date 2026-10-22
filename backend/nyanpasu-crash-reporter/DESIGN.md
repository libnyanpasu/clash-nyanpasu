# Crash reporter design

The panicking process cannot safely run another UI event loop. A child
process receives the panic over stdin and owns the report window. The host
keeps its startup-error dialogs and a short rfd fallback if Slint fails.

- `src/report.rs`: the JSON handoff, plain-text report and encoded issue URL.
  Long backtraces lose whole lines from the end to fit the 6000-byte URL
  budget; the clipboard always carries the full report.
  If metadata alone exceeds that budget, the issue URL is unavailable and
  the action shows the localized failure status after copying the report.
- `src/lib.rs`: localized strings, window setup, clipboard actions, the
  three-second copy status timer and browser launch. Copying goes through
  `arboard` rather than the TextEdit's select-all/copy, which would move the
  cursor and scroll the report to its end.
- `src/platform.rs`: software winit backend and the monospace family from the frontend's stack.
- `ui/reporter.slint`: Fluent layout, palette, selection and keyboard handling.
- `src/render_tests.rs`: headless rendering in every locale, both themes and the minimum size.

The backend is selected directly to avoid the muda 0.19/0.20 Objective-C
class collision described in `nyanpasu-widget/Cargo.toml`. The opaque window
needs neither a GL driver nor the widget's taskbar, Dock or topmost overrides.
Slint takes a single font family and falls back to a sans-serif font, so
`platform.rs` resolves the frontend's `--font-mono` stack against the system
fonts with fontique, the library Slint resolves fonts with, and passes the
first installed family. The Fluent palette follows the system theme;
only the critical icon uses explicit light/dark colours.
