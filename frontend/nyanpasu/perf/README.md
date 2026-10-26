# Page benchmarks

Benchmarks that render real page components against fixture data in a headless
browser and measure frame drops. They run only on demand, never in `pnpm test`.

```sh
pnpm -F @nyanpasu/nyanpasu bench
```

Each test prints one `PERF_REPORT {...}` line, averaged over the measured
actions after a warm-up (the connections benchmark reports page openings under
`open`, search keystrokes under `keystroke` and streamed frames under `frame`;
the profiles benchmark reports the list opened from another section under
`open`, moves to and from a detail page under `detail` and `list`, and
`unchangedRefetch`; the proxies benchmark prints a line for group switches,
`switch`, and one for the page opened on a group from another section, `open`;
the settings benchmark prints a line per page and `open:<page>` for the
settings opened on their first page from another section; the traffic
benchmark prints a line per action: `open:<page>`, `poll:<page>` for a polled
report arriving, `switch:<page>` for switching to the page from the overview,
`append:rankings` for the next page of the listing and `filter:overview` for a
ranking row toggling its filter):

| Field             | Meaning                                                                       |
| ----------------- | ----------------------------------------------------------------------------- |
| `avgMaxFrameGap`  | Longest gap between two animation frames per action; ~16.7 ms is smooth       |
| `avgFramesOver50` | Frames per action that took over 50 ms                                        |
| `avgLongTaskMax`  | Longest task the browser reported per action (Chromium only)                  |
| `avgMaxBusy`      | Longest stretch the main thread was busy per action, rendering included       |
| `avgSyncTime`     | Synchronous JavaScript from the action to its first DOM mutation              |
| `avgReactRender`  | React render time per action, summed over commits (excludes the commit phase) |

Frame gaps never read below a frame at the browser's 60 Hz, and the work of
the action's own frame does not always show in them; check a frame budget,
such as 8.3 ms at 120 Hz, against `avgMaxBusy`. An idle page reads about 3 ms.
It reads 0 while `VITE_PERF_PROFILE` records a profile, which would otherwise
mostly record the measurement.

Frame timing is noisy. On a CPU with performance and efficiency cores, the
scheduler moves the browser between them, so the same build can run two or
three times slower from one minute to the next. Compare variants in
alternating runs pinned to one kind of core, and read changes smaller than a
frame with suspicion. From the repository root,

```sh
VITE_PERF_THROTTLE=4 deno task bench:ab --rounds 3 --cores performance traffic
```

runs a benchmark without (A) and with (B) the working tree's frontend changes,
in turn, and prints each variant's range over the rounds and the median of
B over A within a round. `--cores performance` runs on one thread of each
performance core, `--cores efficiency` on the efficiency cores, which stand in
for a slow machine (Windows and Linux; a CPU without hybrid cores uses all of
them either way). `--base` picks the revision A is, `HEAD` by default; changes
to the benchmarks themselves are part of the comparison, so commit those
first, and stage new files with `git add -N` so the comparison sees them. The
working tree ends with the changes applied.

## Options

| Variable                   | Default       | Effect                                                                            |
| -------------------------- | ------------- | --------------------------------------------------------------------------------- |
| `PERF_BROWSER`             | `chromium`    | `webkit` runs the engine macOS and Linux webviews use                             |
| `PERF_DEV`                 | unset         | `1` uses development React instead of the production build                        |
| `PERF_TRACE`               | unset         | `1` records Chromium traces where `VITE_PERF_PROFILE` records a CPU profile       |
| `VITE_PERF_THROTTLE`       | `1`           | CPU slowdown factor (Chromium only), e.g. `4` for a slow machine                  |
| `VITE_PERF_GROUPS`         | `30`          | Proxy groups in the fixture                                                       |
| `VITE_PERF_NODES`          | `1500`        | Nodes in the fixture; every group lists all of them                               |
| `VITE_PERF_SWITCHES`       | `12`          | Measured actions, including 2 warm-ups                                            |
| `VITE_PERF_LOGS`           | `1024`        | Logs in the logs fixture (`200`, one page, for a log file)                        |
| `VITE_PERF_SOURCE`         | `core`        | Logs page source: `core` history or the `app` log file                            |
| `VITE_PERF_PROFILES`       | `30`          | Remote subscriptions in the profiles fixture                                      |
| `VITE_PERF_PAGE`           | `connections` | Page the connection stream feeds: `connections` or `rules`                        |
| `VITE_PERF_CONNECTIONS`    | `2000`        | Connections in each streamed frame; 1% are replaced per frame                     |
| `VITE_PERF_RULES`          | `3000`        | Rules in the rules fixture (`10000` in the inspector benchmark)                   |
| `VITE_PERF_FRESH`          | unset         | `1` shows the inspector's YAML as a new document each switch, never cached        |
| `VITE_PERF_FRAMES`         | `12`          | Measured connection frames, one a second, including 2 warm-ups                    |
| `VITE_PERF_PAGES`          | all           | Pages to measure: settings `clash,system`, traffic `overview,flow,map,rankings`   |
| `VITE_PERF_TRAFFIC_GROUPS` | `1000`        | Hosts the traffic rankings page lists, 50 per loaded page                         |
| `VITE_PERF_SEARCH`         | unset         | A search term typed key by key, each keystroke measured                           |
| `VITE_PERF_SORT`           | unset         | A column header to sort by before the frames, e.g. `DL Speed`                     |
| `VITE_PERF_DETAIL`         | unset         | `1` keeps a connection's detail dialog open during the frames                     |
| `VITE_PERF_PRINT_GAPS`     | unset         | `1` prints every frame gap of every action                                        |
| `VITE_PERF_PROFILE`        | unset         | A path to write a `.cpuprofile` of 6 more actions (Chromium only)                 |
| `VITE_PERF_PROFILE_ACTION` | `open`        | Traffic: `poll` profiles six polls of the first page, `switch` six switches to it |
| `VITE_PERF_TRACE_REFLOW`   | unset         | `1` logs every layout read of 2 ms or more with its call site                     |
| `VITE_PERF_RENDERS`        | unset         | `1` adds the components each action rendered to its report                        |

## Finding where the time goes

- `VITE_PERF_RENDERS=1` adds `renders` to each report, per action: its
  commits, the components every commit rendered as `[mounts, updates]`
  (`all`), those of the commit that rendered the most (`largest`), and the
  components that rendered while their parent did not, with their nearest
  parents (`unprompted`): a state, store or context update starts there.
  Counting walks the fiber tree after each commit, so compare the timings of
  such runs only with each other.
- `VITE_PERF_PROFILE` records a CPU profile, which opens in Chrome DevTools'
  Performance panel. From the repository root,
  `deno task bench:profile --actions 6 <file>` prints its time per action by
  category, source and function, and `--callers <function>` the stacks under
  one function's own time, such as `getBoundingClientRect` for a forced
  layout.
- With `PERF_TRACE=1` as well, the same actions record a Chromium trace,
  which splits what a profile shows as `(program)` into style, layout, paint
  and compositing. A recording paused in between, such as the traffic
  switches', is written one trace per piece: `<name>-1.json`, ...
  `deno task bench:trace <files>` prints each trace's longest main-thread
  tasks and the time of each step in them.
- WebKit has neither; use `VITE_PERF_TRACE_REFLOW` and `avgSyncTime` to split
  its time between JavaScript and style/layout.

## Adding a benchmark

Name it `*.bench.test.tsx`. Mock the `@nyanpasu/query` hooks the page reads
with stable fixture objects, mount the page's routes with the ids and paths from
`route-tree.gen.ts`, and measure actions with `measureFrames` from `measure.ts`.
