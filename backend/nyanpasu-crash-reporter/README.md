# nyanpasu-crash-reporter

A Fluent Slint window for crash and manual issue reports: selectable report text,
diagnostic progress, clipboard copy, a prefilled GitHub issue, and archive reveal.
It uses the software renderer on every platform and has no Tauri dependency.

`show_report(ReporterInput, &ReporterText, Receiver<Event>)` runs once on the main
thread. The host supplies localized strings and feeds `nyanpasu-diagnose` events.
`clash-nyanpasu panic-dialog` reads `PanicDetails` JSON (including `parent_pid`) to
EOF and collects on a thread. `clash-nyanpasu report-issue` reads event JSON lines
from the running GUI parent. The reporter uploads nothing. The user attaches the
revealed archive to the issue manually.

Both reporter commands accept `--language <key>` using the application's language
keys. The GUI passes its selected language; standalone commands that omit the
option use the OS UI language.

`cargo test -p nyanpasu-crash-reporter` covers report formatting, issue encoding
and URL budgets, JSON, and headless crash/manual rendering with progress, finished
and failed states. A deterministic sparse-font test checks that every regression
character paints ink through CJK fallback. Set `NYANPASU_DIAGNOSE_PREVIEW_DIR` to
write light/dark PNGs for all six locales from the rendering test.

See [DESIGN.md](DESIGN.md) for ownership and rendering choices.
