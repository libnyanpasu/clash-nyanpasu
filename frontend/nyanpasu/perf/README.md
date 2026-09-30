# Page benchmarks

Benchmarks that render real page components against fixture data in a headless
browser and measure frame drops. They run only on demand, never in `pnpm test`.

```sh
pnpm -F @nyanpasu/nyanpasu bench
```

Each test prints one `PERF_REPORT {...}` line, averaged over the measured
actions after a warm-up:

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

| Variable                 | Default    | Effect                                                            |
| ------------------------ | ---------- | ----------------------------------------------------------------- |
| `PERF_BROWSER`           | `chromium` | `webkit` runs the engine macOS and Linux webviews use             |
| `PERF_DEV`               | unset      | `1` uses development React instead of the production build        |
| `VITE_PERF_THROTTLE`     | `1`        | CPU slowdown factor (Chromium only), e.g. `4` for a slow machine  |
| `VITE_PERF_GROUPS`       | `30`       | Proxy groups in the fixture                                       |
| `VITE_PERF_NODES`        | `1500`     | Nodes in the fixture; every group lists all of them               |
| `VITE_PERF_SWITCHES`     | `12`       | Measured actions, including 2 warm-ups                            |
| `VITE_PERF_PRINT_GAPS`   | unset      | `1` prints every frame gap of every action                        |
| `VITE_PERF_PROFILE`      | unset      | A path to write a `.cpuprofile` of 6 more actions (Chromium only) |
| `VITE_PERF_TRACE_REFLOW` | unset      | `1` logs every layout read of 2 ms or more with its call site     |

A `.cpuprofile` opens in Chrome DevTools' Performance panel. WebKit has no
profiler here; use `VITE_PERF_TRACE_REFLOW` and `avgSyncTime` to split its time
between JavaScript and style/layout.

## Adding a benchmark

Name it `*.bench.test.tsx`. Mock the `@nyanpasu/interface` hooks the page reads
with stable fixture objects, mount the page's routes with the ids and paths from
`route-tree.gen.ts`, and measure actions with `measureFrames` from `measure.ts`.
