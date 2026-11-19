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
- `src/platform.rs`: software winit backend and the CJK fallback chains. Centring,
  monospace-family resolution and the system's wheel lines come from
  [`nyanpasu-slint`](https://github.com/libnyanpasu/nyanpasu-slint), shared with
  the installer.
- `ui/reporter.slint`: Fluent palette, conditional crash details, progress/done row,
  the report in `nyanpasu-slint`'s selectable, unwrapped monospace `LogView`, and
  keyboard handling. Each pane gets its own view, so a pane opens at its top-left
  corner rather than where the other one was scrolled; within a pane the
  position stays as the environment arrives.
- `src/render_tests.rs`: headless software snapshots in each locale, both themes,
  scales 1/2 and the minimum size, plus crash/manual progress at 40%, finished and
  failed-row checks, and the report's hover copy button, wheel scrolling, pane
  reset, exact Ctrl+A/Ctrl+C copy and Escape from inside it. PNG output is
  environment-gated.
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

The startup details use the same `LogView`. Unlike Slint's read-only `TextEdit`,
it has no cursor that scrolls to the end of new text, so neither window resets
its selection or scroll position, and a wheel notch repaints once instead of
animating for some 28 frames. The host sends one JSON document then closes stdin explicitly before waiting;
the startup child reads to EOF. It never starts diagnostics collection or opens
the application's stores. Failure classification, available backup information,
paths and translations belong to the host, not to this renderer.

The backend is selected directly to avoid the muda 0.21/0.20 Objective-C class
collision described in `nyanpasu-widget/Cargo.toml`. The opaque window needs no GL
driver. Slint's shared font collection is configured before creating the window:
installed CJK families extend its Han, Hiragana, Katakana and Hangul fallback
chains. This preserves the system's first choice while covering characters that
its chosen family lacks, including mixed-language report text. The fallback code
uses Slint's public versioned Fontique API; the exact Slint pin, shared with
`nyanpasu-slint`, must be updated together with that API when upgrading.
The Fluent palette follows the system theme.

Collection policy lives in `nyanpasu-diagnose`; the reporter never opens the app's
core store or discovers its application paths. The host owns the parent-PID wait,
core-version reader and typed config loading. See the diagnosis
[design](../nyanpasu-diagnose/DESIGN.md) for the recorded plan deviations.
