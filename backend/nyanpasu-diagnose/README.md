# nyanpasu-diagnose

A frontend-independent diagnosis: host facts, ordered environment text, and a
redacted zip of the last three days of app and core logs. A synchronous run emits
`Started`, per-step `Progress`, and `Finished` events. Failed steps become problems;
the remaining steps still run. Nothing is uploaded.

The desktop host supplies webview and core facts, build pairs, page capabilities,
paths and core-log export closures. Crash children and manual issue reports share
the same run; the CLI can omit archive steps. The Slint reporter consumes the
serializable events without owning collection policy.

Run `cargo test -p nyanpasu-diagnose` from `backend/` for host grouping, the ported
environment formatter cases, progress/error continuation, redaction, archive caps,
age filtering and retention. See [DESIGN.md](DESIGN.md) for ownership and limits.
