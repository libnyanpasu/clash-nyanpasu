import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import { type CoreKind, pinCores } from "./cpu-cores.ts";

/** The fields of a `PERF_REPORT` line the comparison lists. */
export const FIELDS = ["avgReactRender", "avgMaxBusy", "avgSyncTime"] as const;

type Field = typeof FIELDS[number];

export type Variant = "A" | "B";

/** One benchmark run's report of one action. */
export type Report = { action: string } & Record<Field, number>;

/** Reads the `PERF_REPORT` lines a benchmark run printed. */
export function parseReports(output: string): Report[] {
  return output
    .split("\n")
    .map((line) => line.match(/PERF_REPORT (\{.*\})/)?.[1])
    .filter((json): json is string => json !== undefined)
    .map((json) => JSON.parse(json) as Report);
}

/**
 * The range each variant's runs spanned, per action and field, and the median
 * of B over A within each round, as `action field: A min–max | B min–max |
 * B/A median`. The machine's speed drifts between rounds more than within
 * one, so the ratio is what tells the variants apart. Actions keep their
 * first order.
 */
export function compare(runs: { variant: Variant; reports: Report[] }[]) {
  const ranges = new Map<string, Record<Variant, number[]>>();

  for (const { variant, reports } of runs) {
    for (const report of reports) {
      for (const field of FIELDS) {
        const key = `${report.action} ${field}`;
        const range = ranges.get(key) ?? { A: [], B: [] };
        range[variant].push(report[field]);
        ranges.set(key, range);
      }
    }
  }

  const span = (values: number[]) =>
    values.length === 0 ? "-" : `${Math.min(...values)}–${Math.max(...values)}`;

  const ratio = ({ A, B }: Record<Variant, number[]>) => {
    const ratios = A.flatMap((a, round) =>
      a > 0 && round < B.length ? [B[round] / a] : []
    ).sort((x, y) => x - y);
    if (ratios.length === 0) return "-";
    const middle = Math.floor(ratios.length / 2);
    const median = ratios.length % 2
      ? ratios[middle]
      : (ratios[middle - 1] + ratios[middle]) / 2;
    return median.toFixed(2);
  };

  return [...ranges].map(([key, range]) =>
    `${key}: A ${span(range.A)} | B ${span(range.B)} | B/A ${ratio(range)}`
  );
}

async function git(args: string[]): Promise<string> {
  const result = await new Deno.Command("git", {
    args,
    cwd: WORKSPACE_ROOT,
    stdout: "piped",
    stderr: "inherit",
  }).output();
  if (!result.success) {
    throw new Error(`git ${args[0]} failed (${result.code})`);
  }

  return new TextDecoder().decode(result.stdout);
}

/** Applies the change (B) or reverses it (A) in the working tree. */
function apply(patch: string, variant: Variant) {
  const child = new Deno.Command("git", {
    args: ["apply", ...(variant === "A" ? ["-R"] : []), "-"],
    cwd: WORKSPACE_ROOT,
    stdin: "piped",
    stderr: "inherit",
  }).spawn();
  const writer = child.stdin.getWriter();

  return writer.write(new TextEncoder().encode(patch))
    .then(() => writer.close())
    .then(() => child.status)
    .then((status) => {
      if (!status.success) {
        throw new Error(`git apply failed for variant ${variant}`);
      }
    });
}

/** The command that runs a page benchmark, pinned to `cores` if given. */
function benchCommand(filter: string, cores?: CoreKind) {
  const args = ["pnpm", "-F", "@nyanpasu/nyanpasu", "bench", filter];
  // pnpm is a `.cmd` or an `.exe` on Windows, which only the shell resolves alike.
  const command = Deno.build.os === "windows" ? ["cmd", "/c", ...args] : args;

  return cores ? [...pinCores(cores), ...command] : command;
}

async function bench([command, ...args]: string[], filter: string) {
  const result = await new Deno.Command(command, {
    args,
    cwd: WORKSPACE_ROOT,
    stdout: "piped",
    stderr: "inherit",
  }).output();
  const output = new TextDecoder().decode(result.stdout);
  if (!result.success) {
    console.error(output);
    throw new Error(`benchmark ${filter} failed (${result.code})`);
  }

  return output;
}

/**
 * Runs a page benchmark alternately without (A) and with (B) the working
 * tree's frontend changes against a revision, so both variants share the
 * machine's state, and prints the range of each over the rounds. With
 * `--cores`, every run is pinned to the performance or efficiency cores, so
 * the scheduler cannot move the browser between them mid-run.
 */
export async function main(args: string[]): Promise<void> {
  const parsed = parseArgs(args, {
    string: ["rounds", "base", "cores"],
    default: { rounds: "3", base: "HEAD" },
  });
  const filter = parsed._[0];
  const rounds = Number(parsed.rounds);
  const cores = parsed.cores;
  if (
    typeof filter !== "string" || parsed._.length !== 1 ||
    !Number.isInteger(rounds) || rounds < 1 ||
    (cores !== undefined && cores !== "performance" && cores !== "efficiency")
  ) {
    throw new Error(
      "Usage: deno task bench:ab [--rounds 3] [--base HEAD] " +
        "[--cores performance|efficiency] <benchmark>",
    );
  }
  const command = benchCommand(filter, cores);

  const patch = await git(["diff", "--binary", parsed.base, "--", "frontend"]);
  if (patch === "") {
    throw new Error(`the frontend has no changes against ${parsed.base}`);
  }

  const runs: { variant: Variant; reports: Report[] }[] = [];
  let current: Variant = "B";
  try {
    for (let round = 1; round <= rounds; round++) {
      for (const variant of ["A", "B"] as const) {
        if (variant !== current) {
          await apply(patch, variant);
          current = variant;
        }
        const reports = parseReports(await bench(command, filter));
        console.log(`== ${variant}${round}`);
        reports.forEach((report) => console.log(JSON.stringify(report)));
        runs.push({ variant, reports });
      }
    }
  } finally {
    // The working tree always ends with the change applied.
    if (current === "A") await apply(patch, "B");
  }

  console.log(
    `\n${rounds} rounds, A = ${parsed.base}, B = working tree` +
      (cores ? `, on the ${cores} cores` : ""),
  );
  compare(runs).forEach((line) => console.log(line));
}

if (import.meta.main) {
  main(Deno.args).catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
