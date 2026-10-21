import { parseArgs } from "jsr:@std/cli@1/parse-args";

/** A V8 CPU profile, as Chromium's profiler writes it. */
export type CpuProfile = {
  nodes: {
    id: number;
    callFrame: { functionName: string; url: string; lineNumber: number };
    children?: number[];
  }[];
  samples: number[];
  timeDeltas: number[];
};

type Node = CpuProfile["nodes"][number];

// Samples outside JavaScript. Under CPU throttling most of "(program)" is the
// throttling itself; a trace splits the rest into style, layout and paint.
const CATEGORIES = ["(idle)", "(program)", "(garbage collector)"];

/**
 * Where a frame's code comes from: the app's file under `src/` (or the
 * benchmark's under `perf/`), another workspace package's as `ui/button.tsx`,
 * the pre-bundled module's name for a dependency (`motion_react`,
 * `react-dom_client`), a package's name under `node_modules`, or `(native)`.
 */
export function sourceOf(url: string): string {
  if (url === "") return "(native)";
  const path = new URL(url, "http://localhost").pathname;

  const dependency = path.match(/\/deps\/([^/]+)\.js$/)?.[1];
  if (dependency) return dependency;
  const workspace = path.match(/\/frontend\/([^/]+)\/(src|perf)\/(.+)$/);
  if (workspace) {
    const [, pack, folder, file] = workspace;
    if (pack !== "nyanpasu") return `${pack}/${file}`;
    return folder === "src" ? file : `perf/${file}`;
  }
  const app = path.match(/^\/(src|perf)\/(.+)$/);
  if (app) return app[1] === "src" ? app[2] : `perf/${app[2]}`;
  const pack = path.match(/.*\/node_modules\/((?:@[^/]+\/)?[^/]+)\//)?.[1];

  return pack ?? path.split("/").at(-1)!;
}

const frameName = ({ callFrame }: Node) =>
  `${callFrame.functionName || "(anonymous)"} ${sourceOf(callFrame.url)}:${
    callFrame.lineNumber + 1
  }`;

/** The time, in milliseconds, each node spent itself. */
function selfTimes(profile: CpuProfile): Map<number, number> {
  const times = new Map<number, number>();
  profile.samples.forEach((node, index) => {
    times.set(node, (times.get(node) ?? 0) + profile.timeDeltas[index] / 1000);
  });

  return times;
}

const top = (totals: Map<string, number>, count: number, per: number) =>
  [...totals]
    .sort(([, a], [, b]) => b - a)
    .slice(0, count)
    .map(([name, ms]) => ({ name, ms: Number((ms / per).toFixed(2)) }));

const addTo = (totals: Map<string, number>, key: string, ms: number) =>
  totals.set(key, (totals.get(key) ?? 0) + ms);

/**
 * A profile's time per action: outside JavaScript by category, and the
 * script's own time by source and by function, the largest first.
 */
export function summarizeProfile(
  profile: CpuProfile,
  { actions = 1, count = 25 }: { actions?: number; count?: number } = {},
) {
  const nodes = new Map(profile.nodes.map((node) => [node.id, node]));
  const categories = new Map<string, number>();
  const sources = new Map<string, number>();
  const functions = new Map<string, number>();

  for (const [id, ms] of selfTimes(profile)) {
    const node = nodes.get(id)!;
    const name = node.callFrame.functionName;
    if (CATEGORIES.includes(name)) {
      addTo(categories, name, ms);
      continue;
    }
    if (name === "(root)") continue;
    addTo(categories, "script", ms);
    addTo(sources, sourceOf(node.callFrame.url), ms);
    addTo(functions, frameName(node), ms);
  }

  return {
    categories: top(categories, CATEGORIES.length + 1, actions),
    sources: top(sources, count, actions),
    functions: top(functions, count, actions),
  };
}

/**
 * The call stacks under which a function spent its own time, `depth` frames
 * each from its caller outwards, the largest first.
 */
export function callersOf(
  profile: CpuProfile,
  functionName: string,
  { actions = 1, count = 8, depth = 6 } = {},
) {
  const nodes = new Map(profile.nodes.map((node) => [node.id, node]));
  const parents = new Map<number, number>();
  for (const node of profile.nodes) {
    for (const child of node.children ?? []) parents.set(child, node.id);
  }
  const stacks = new Map<string, number>();

  for (const [id, ms] of selfTimes(profile)) {
    if (nodes.get(id)!.callFrame.functionName !== functionName) continue;
    const frames = [];
    for (
      let parent = parents.get(id);
      parent !== undefined && frames.length < depth;
      parent = parents.get(parent)
    ) {
      frames.push(frameName(nodes.get(parent)!));
    }
    addTo(stacks, frames.join(" < "), ms);
  }

  return top(stacks, count, actions);
}

const table = (rows: { name: string; ms: number }[]) =>
  rows.map(({ name, ms }) => `${ms.toFixed(2).padStart(9)}  ${name}`);

/**
 * Prints where a benchmark's CPU profile (`VITE_PERF_PROFILE`) spent its
 * time, per measured action with `--actions`, or the stacks under which one
 * function did with `--callers`.
 */
export async function main(args: string[]): Promise<void> {
  const parsed = parseArgs(args, {
    string: ["actions", "top", "callers"],
    default: { actions: "1", top: "25" },
  });
  const actions = Number(parsed.actions);
  const count = Number(parsed.top);
  const [file] = parsed._;
  if (
    typeof file !== "string" || parsed._.length !== 1 ||
    !(actions > 0) || !Number.isInteger(count) || count < 1
  ) {
    throw new Error(
      "Usage: deno task bench:profile [--actions 1] [--top 25] " +
        "[--callers <function>] <file.cpuprofile>",
    );
  }
  const profile = JSON.parse(await Deno.readTextFile(file)) as CpuProfile;
  const unit = actions === 1 ? "ms" : "ms per action";

  if (parsed.callers) {
    console.log(`${parsed.callers}'s own time by caller (${unit}):`);
    callersOf(profile, parsed.callers, { actions, count }).forEach((row) =>
      console.log(`${row.ms.toFixed(2).padStart(9)}  ${row.name}\n`)
    );
    return;
  }

  const summary = summarizeProfile(profile, { actions, count });
  for (
    const [title, rows] of [
      ["By category", summary.categories],
      ["Script by source", summary.sources],
      ["Script by function", summary.functions],
    ] as const
  ) {
    console.log(`${title} (${unit}):`);
    table(rows).forEach((line) => console.log(line));
    console.log();
  }
}

if (import.meta.main) {
  main(Deno.args).catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
