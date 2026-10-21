import { assertEquals } from "jsr:@std/assert@1";
import { longestTasks, mainThread, type TraceEvent } from "./trace-report.ts";

const slice = (tid: number, name: string, ts: number, dur: number) => ({
  name,
  ph: "X",
  pid: 1,
  tid,
  ts,
  dur,
});

const events: TraceEvent[] = [
  {
    name: "thread_name",
    ph: "M",
    pid: 1,
    tid: 1,
    ts: 0,
    args: { name: "CrRendererMain" },
  },
  {
    name: "thread_name",
    ph: "M",
    pid: 1,
    tid: 2,
    ts: 0,
    args: { name: "Compositor" },
  },
  // A 10 ms task: 6 ms of script, of which 2 ms forced a layout, then a
  // 3 ms layout and a paint.
  slice(1, "RunTask", 0, 10_000),
  slice(1, "FunctionCall", 0, 6_000),
  slice(1, "Layout", 4_000, 2_000),
  slice(1, "Layout", 6_000, 3_000),
  slice(1, "Paint", 9_000, 1_000),
  slice(1, "RunTask", 20_000, 2_000),
  slice(2, "RunTask", 0, 50_000),
];

Deno.test("longestTasks splits a main-thread task into its steps", () => {
  assertEquals(longestTasks(mainThread(events), 1), [
    {
      ms: 10,
      steps: [
        { name: "Layout", ms: 5 },
        { name: "FunctionCall", ms: 4 },
        { name: "Paint", ms: 1 },
      ],
    },
  ]);
});

Deno.test("mainThread leaves the other threads out", () => {
  assertEquals(
    longestTasks(mainThread(events), 3).map(({ ms }) => ms),
    [10, 2],
  );
});
