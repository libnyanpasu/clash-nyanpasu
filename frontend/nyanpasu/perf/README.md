# Page benchmarks

Benchmarks that render real page components against fixture data in a headless
browser and measure frame drops. They run only on demand, never in `pnpm test`.

```sh
pnpm -F @nyanpasu/nyanpasu bench
```

Each test prints one `PERF_REPORT {...}` line, averaged over the measured
actions after a warm-up (the connections benchmark reports page openings under
`open`, search keystrokes under `keystroke` and streamed frames under `frame`):

| Field             | Meaning                                                                       |
| ----------------- | ----------------------------------------------------------------------------- |
| `avgMaxFrameGap`  | Longest gap between two animation frames per action; ~16.7 ms is smooth       |
| `avgFramesOver50` | Frames per action that took over 50 ms                                        |
| `avgLongTaskMax`  | Longest task the browser reported per action (Chromium only)                  |
| `avgSyncTime`     | Synchronous JavaScript from the action to its first DOM mutation              |
| `avgReactRender`  | React render time per action, summed over commits (excludes the commit phase) |

Frame timing is noisy: compare runs on the same machine, run each variant at
least twice, and read changes smaller than a frame with suspicion.

## Options

| Variable                 | Default       | Effect                                                            |
| ------------------------ | ------------- | ----------------------------------------------------------------- |
| `PERF_BROWSER`           | `chromium`    | `webkit` runs the engine macOS and Linux webviews use             |
| `PERF_DEV`               | unset         | `1` uses development React instead of the production build        |
| `VITE_PERF_THROTTLE`     | `1`           | CPU slowdown factor (Chromium only), e.g. `4` for a slow machine  |
| `VITE_PERF_GROUPS`       | `30`          | Proxy groups in the fixture                                       |
| `VITE_PERF_NODES`        | `1500`        | Nodes in the fixture; every group lists all of them               |
| `VITE_PERF_SWITCHES`     | `12`          | Measured actions, including 2 warm-ups                            |
| `VITE_PERF_LOGS`         | `1024`        | Logs in the logs fixture (`200`, one page, for a log file)        |
| `VITE_PERF_SOURCE`       | `core`        | Logs page source: `core` history or the `app` log file            |
| `VITE_PERF_PROFILES`     | `30`          | Remote subscriptions in the profiles fixture                      |
| `VITE_PERF_PAGE`         | `connections` | Page the connection stream feeds: `connections` or `rules`        |
| `VITE_PERF_CONNECTIONS`  | `2000`        | Connections in each streamed frame; 1% are replaced per frame     |
| `VITE_PERF_RULES`        | `3000`        | Rules in the rules fixture (`10000` in the inspector benchmark)   |
| `VITE_PERF_FRAMES`       | `12`          | Measured connection frames, one a second, including 2 warm-ups    |
| `VITE_PERF_SEARCH`       | unset         | A search term typed key by key, each keystroke measured           |
| `VITE_PERF_SORT`         | unset         | A column header to sort by before the frames, e.g. `DL Speed`     |
| `VITE_PERF_DETAIL`       | unset         | `1` keeps a connection's detail dialog open during the frames     |
| `VITE_PERF_PRINT_GAPS`   | unset         | `1` prints every frame gap of every action                        |
| `VITE_PERF_PROFILE`      | unset         | A path to write a `.cpuprofile` of 6 more actions (Chromium only) |
| `VITE_PERF_TRACE_REFLOW` | unset         | `1` logs every layout read of 2 ms or more with its call site     |

A `.cpuprofile` opens in Chrome DevTools' Performance panel. WebKit has no
profiler here; use `VITE_PERF_TRACE_REFLOW` and `avgSyncTime` to split its time
between JavaScript and style/layout.

## Adding a benchmark

Name it `*.bench.test.tsx`. Mock the `@nyanpasu/interface` hooks the page reads
with stable fixture objects, mount the page's routes with the ids and paths from
`route-tree.gen.ts`, and measure actions with `measureFrames` from `measure.ts`.
