# nyanpasu-crash-reporter

A Fluent Slint window for a fatal panic: a scrollable, selectable report,
clipboard copy, and a prefilled GitHub issue. It uses the software renderer
on every platform and has no Tauri dependency.

The application starts `clash-nyanpasu panic-dialog` and writes `PanicDetails`
as JSON on its stdin. The child reads to EOF before opening the window and
survives the parent's exit. The host supplies build information, OS and
localized strings; the reporter uploads nothing.

`cargo test -p nyanpasu-crash-reporter` covers report formatting, issue query
encoding and backtrace truncation, JSON, and headless rendering in both
themes at scale 1 and 2.

See [DESIGN.md](DESIGN.md) for ownership and rendering choices.
