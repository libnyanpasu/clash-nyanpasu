# Crash reporter design

The panicking process cannot safely run another UI event loop. A child process
receives the panic over stdin and owns the report window. Manual reports use the
same child and window, with events streamed by the running GUI. Startup failures
use a separate `startup-failed` child and window. The host keeps one-shot rfd
dialogs for pre-builder environment failures and as a fallback if Slint fails.

- `src/report.rs`: `PanicDetails`, `CrashReport`, `ReporterInput` and encoded issue
  URLs. Crash URLs drop whole backtrace lines to fit 6000 bytes. Manual URLs have
  an empty title and environment fields, without crash-only actual/log fields.
  Oversized metadata leaves the report copyable and shows the open-failed status.
- `src/lib.rs`: localized strings, event receiver thread, UI-thread event updates,
  weighted progress, completion/problem status, clipboard, issue launch and reveal.
  A disconnected sender before `Finished` displays an interruption; Report issue
  stays disabled. Copy and Close work throughout collection. Each click copies
  the current full report through arboard, preserving scroll and the X11 clipboard
  owner's lifetime. The three-second copied timer is retained.
- `src/platform.rs`: software winit backend, centring, monospace-family resolution,
  and selected-file reveal on Windows/macOS (directory opening on Linux).
- `ui/reporter.slint`: Fluent palette, conditional crash details, progress/done row,
  read-only monospace TextEdit and keyboard handling. The first-layout reset and
  explicit selection reset are retained. Appending saves `content-y` and restores
  it in the deferred text-change callback, after the cursor reset.
- `src/render_tests.rs`: headless software snapshots in each locale, both themes,
  scales 1/2 and the minimum size, plus crash/manual progress at 40%, finished,
  failed-row and append-scroll checks. PNG output is environment-gated.
- `src/startup_failure.rs` and `ui/startup-failure.slint`: host-supplied heading,
  description, recovery, version, path and copyable details. Optional containing,
  backups and logs folder actions stay in the window; only Exit/Escape or the
  window close control ends it. Folder/copy failures set an inline status.
  The clipboard owner remains alive through the window lifetime. Backend,
  centring, theme and font selection are shared with the crash reporter.
- `src/startup_failure_render_tests.rs`: runs on the existing headless UI thread,
  asserts localized text, folder visibility and action callbacks in all six
  locales, both themes, scales 1/2 and minimum size. Set
  `NYANPASU_STARTUP_PREVIEW_DIR` to save the English and Simplified Chinese
  light/dark PNGs for migration, user config, runtime directory and internal
  failures. Paths are deliberately mixed Latin/CJK, independent of OS language.

The startup TextEdit retains the crash window's two layout safeguards: reset
selection when assigning text, and reset scroll after the first nonzero layout.
Setting scroll before layout alone lets the initial cursor move it to the end.
The host sends one JSON document then closes stdin explicitly before waiting;
the startup child reads to EOF. It never starts diagnostics collection or opens
the application's stores. Failure classification, available backup information,
paths and translations belong to the host, not to this renderer.

The backend is selected directly to avoid the muda 0.19/0.20 Objective-C class
collision described in `nyanpasu-widget/Cargo.toml`. The opaque window needs no GL
driver. Slint's shared font collection is configured before creating the window:
installed CJK families extend its Han, Hiragana, Katakana and Hangul fallback
chains. This preserves the system's first choice while covering characters that
its chosen family lacks, including mixed-language report text. Monospace-family
resolution uses the same Fontique version through Slint's public versioned API;
the exact Slint pin must be updated together with that API when upgrading.
The Fluent palette follows the system theme.

Collection policy lives in `nyanpasu-diagnose`; the reporter never opens the app's
core store or discovers its application paths. The host owns the parent-PID wait,
core-version reader and typed config loading. See the diagnosis
[design](../nyanpasu-diagnose/DESIGN.md) for the recorded plan deviations.
