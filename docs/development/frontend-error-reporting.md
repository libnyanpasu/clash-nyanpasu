# Frontend error reporting

Each webview forwards its warnings and errors to the application log, so they
can be analysed with the backend's own records. Nothing leaves the machine.

## Capture

`frontend/nyanpasu/src/services/error-reporting/` is installed by `main.tsx`
before the app renders. It captures:

| Source                     | Kind                                                  | Level                           |
| -------------------------- | ----------------------------------------------------- | ------------------------------- |
| `console.warn` / `.error`  | `console`                                             | warning / error                 |
| `window` `error` (capture) | `uncaught_error`, including failed resource loads     | error                           |
| `unhandledrejection`       | `unhandled_rejection`                                 | error                           |
| React root error options   | `react_uncaught`, `react_caught`, `react_recoverable` | error; recoverable is a warning |

Console output is kept. The React options replace React's default logging,
so they print through the original console methods and are reported once.
`onCaughtError` covers the router's error boundaries.

An event has its message, error name, raw stack, up to three `cause` levels,
the React component stack, the route path and a fingerprint. The fingerprint
hashes the kind, error name, message with numbers, hex strings and UUIDs
replaced, and the first stack frame without query strings. Production builds
have no source maps, so their stacks point into minified bundles.

## Batching and limits

The reporter merges a fingerprint's repeats for 60 seconds: the first
occurrence is sent promptly with its count; a later repeat is held and sent
with its count when the window ends. At most 60 distinct events per minute
are accepted; the rest are counted as dropped. A batch is sent after 1 second
or at 16 events, and when the page is hidden. A failed send is not retried or
logged; its events are counted as dropped in the next batch. Logging done
while the reporter itself runs is not captured, so a failing transport cannot
report itself.

`report_frontend_events` is a mutation on both transports. The backend keeps
no state: it bounds one batch (32 events, 4 KiB messages, 16 KiB stacks, 512 B
names and routes, three causes, cut on character boundaries and flagged
`truncated`), replaces an invalid fingerprint, and writes through the injected
`FrontendLogSink`. The owner is the caller's window label or HTTP session,
never a value the page supplies.

## Log records

`TracingFrontendLogSink` writes each event as one JSON line with target
`clash_nyanpasu::frontend`, so the configured application log level, rotation
and the app log viewer apply. Fields: `message`, `owner`, `kind`,
`fingerprint`, `count`, `route`, `error_name`, `stack`, `causes` (a JSON
string), `component_stack`, `first_seen_ms`, `last_seen_ms` (client clock)
and `truncated`. Dropped events produce one warning with a `dropped` count.
Messages are not scrubbed; anything exported from these logs needs its own
redaction.

## Validation

```sh
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --all-features logs::frontend
pnpm test:frontend error-reporting
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc::tests::browser_debug_page_and_real_rpc -- --exact --ignored --nocapture
```

For manual testing, the debug settings page's Advance Tools include an Error
Reporting Test card with one button per source and a render error that the
route's error boundary catches. The browser fixture needs `pnpm web:build` and
Playwright Chromium; it clicks the console, uncaught error and rejection
buttons and checks that each reaches the sink over HTTP with the server-side
owner.
