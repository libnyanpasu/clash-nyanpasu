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

`cargo test -p nyanpasu-crash-reporter` covers report formatting, issue encoding
and URL budgets, JSON, and headless crash/manual rendering with progress, finished
and failed states. Set `NYANPASU_DIAGNOSE_PREVIEW_DIR` to write English light/dark
and Chinese light PNGs from the rendering test.

See [DESIGN.md](DESIGN.md) for ownership and rendering choices.
