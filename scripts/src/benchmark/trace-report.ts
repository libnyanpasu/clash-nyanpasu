import { parseArgs } from "jsr:@std/cli@1/parse-args";

/** A Chromium trace event, as Playwright's `stopTracing` writes them. */
export type TraceEvent = {
  name: string;
  ph: string;
  pid: number;
  tid: number;
  ts: number;
  dur?: number;
  args?: { name?: string };
};

type Slice = TraceEvent & { dur: number };

// The tasks the renderer's main thread runs, at the top of its slices.
const TASKS = new Set(["RunTask", "ThreadControllerImpl::RunTask"]);

/**
 * The slices of the page's main thread: of the renderer main threads in the
 * trace, the one that ran the most tasks.
 */
export function mainThread(events: TraceEvent[]): Slice[] {
  const threads = new Map<string, Slice[]>();
  const names = new Map<string, string>();

  for (const event of events) {
    const thread = `${event.pid}:${event.tid}`;
    if (event.ph === "M" && event.name === "thread_name") {
      names.set(thread, event.args?.name ?? "");
    } else if (event.ph === "X" && event.dur !== undefined) {
      const slices = threads.get(thread) ?? [];
      slices.push(event as Slice);
      threads.set(thread, slices);
    }
  }

  const tasks = (slices: Slice[]) =>
    slices.filter((slice) => TASKS.has(slice.name)).length;

  return [...threads]
    .filter(([thread]) => names.get(thread) === "CrRendererMain")
    .map(([, slices]) => slices)
    .sort((a, b) => tasks(b) - tasks(a))[0] ?? [];
}

/**
 * The longest tasks of a main thread, each with the time its slices spent
 * themselves by name (`FunctionCall` for script, `UpdateLayoutTree` for
 * style, `Layout`, `Paint`, ...), in milliseconds, the largest first.
 */
export function longestTasks(slices: Slice[], count = 3) {
  const sorted = [...slices].sort((a, b) => a.ts - b.ts || b.dur - a.dur);

  return sorted
    .filter((slice) => TASKS.has(slice.name))
    .sort((a, b) => b.dur - a.dur)
    .slice(0, count)
    .map((task) => {
      const end = task.ts + task.dur;
      const own = new Map<string, number>();
      const open: Slice[] = [];

      for (const slice of sorted) {
        if (
          slice === task || slice.ts < task.ts || slice.ts + slice.dur > end
        ) {
          continue;
        }
        while (open.length && open.at(-1)!.ts + open.at(-1)!.dur <= slice.ts) {
          open.pop();
        }
        const parent = open.at(-1);
        if (parent) {
          own.set(parent.name, (own.get(parent.name) ?? 0) - slice.dur);
        }
        own.set(slice.name, (own.get(slice.name) ?? 0) + slice.dur);
        open.push(slice);
      }

      return {
        ms: task.dur / 1000,
        steps: [...own]
          .filter(([, us]) => us >= 100)
          .sort(([, a], [, b]) => b - a)
          .map(([name, us]) => ({ name, ms: us / 1000 })),
      };
    });
}

/**
 * Prints the longest main-thread tasks of the traces a benchmark recorded
 * with `PERF_TRACE=1`, and what each spent its time on.
 */
export async function main(args: string[]): Promise<void> {
  const parsed = parseArgs(args, {
    string: ["tasks"],
    default: { tasks: "3" },
  });
  const count = Number(parsed.tasks);
  const files = parsed._.map(String);
  if (files.length === 0 || !Number.isInteger(count) || count < 1) {
    throw new Error("Usage: deno task bench:trace [--tasks 3] <trace.json>...");
  }

  for (const file of files) {
    const trace = JSON.parse(await Deno.readTextFile(file));
    const events =
      (Array.isArray(trace) ? trace : trace.traceEvents) as TraceEvent[];
    console.log(file);
    for (const { ms, steps } of longestTasks(mainThread(events), count)) {
      const parts = steps.slice(0, 8).map(({ name, ms }) =>
        `${name} ${ms.toFixed(1)}`
      );
      console.log(`  task ${ms.toFixed(1)} ms: ${parts.join(", ")}`);
    }
  }
}

if (import.meta.main) {
  main(Deno.args).catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
