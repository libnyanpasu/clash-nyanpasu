/** Architecture ledger command entry point. */
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import * as path from "jsr:@std/path";
import { consola } from "../shared/logger.ts";
import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import {
  DEFAULT_ROOTS,
  DEFAULT_SNAPSHOT_PATH,
  isBridgePath,
  type LedgerReport,
  rel,
  type StableSnapshot,
  STATIC_ALLOWLIST,
  staticAllowlistKey,
  type StaticCategory,
} from "./policy.ts";
import {
  collectRustFiles,
  createBuckets,
  scanFile,
  staticAllowlistIssues,
} from "./scan.ts";
import {
  evaluateGate,
  metricsFromBuckets,
  parseStableSnapshot,
  reportToStableSnapshot,
} from "./snapshot.ts";
import { printGateResult, printHuman } from "./report.ts";

async function loadSnapshotFile(
  snapshotPath: string,
): Promise<{ snapshot?: StableSnapshot; error?: string }> {
  const abs = path.isAbsolute(snapshotPath)
    ? snapshotPath
    : path.join(WORKSPACE_ROOT, snapshotPath);
  try {
    const text = await Deno.readTextFile(abs);
    let raw: unknown;
    try {
      raw = JSON.parse(text);
    } catch (err) {
      return {
        error: `invalid snapshot JSON at ${snapshotPath}: ${
          err instanceof Error ? err.message : String(err)
        }`,
      };
    }
    try {
      return { snapshot: parseStableSnapshot(raw) };
    } catch (err) {
      return {
        error: `invalid snapshot contract at ${snapshotPath}: ${
          err instanceof Error ? err.message : String(err)
        }`,
      };
    }
  } catch (err) {
    if (err instanceof Deno.errors.NotFound) {
      return { error: `missing snapshot file: ${snapshotPath}` };
    }
    return {
      error: `failed to read snapshot ${snapshotPath}: ${
        err instanceof Error ? err.message : String(err)
      }`,
    };
  }
}

async function main(): Promise<void> {
  const args = parseArgs(Deno.args, {
    string: ["mode", "root", "format", "snapshot"],
    collect: ["root"],
    default: {
      mode: "report",
      format: "text",
      snapshot: DEFAULT_SNAPSHOT_PATH,
    },
    alias: {
      m: "mode",
      r: "root",
      f: "format",
      s: "snapshot",
      h: "help",
    },
    boolean: ["help", "json", "write-snapshot"],
  });

  if (args.help) {
    console.log(`Usage: deno task architecture-ledger [options]

Options:
  --mode, -m <report|gate>   report (default) always exits 0 after printing.
                             gate loads the committed snapshot, hard-fails on
                             test_real_dirs.total != 0, and exact-compares
                             stable totals/byKey, roots, and bridgeFiles.
  --snapshot, -s <path>      committed stable snapshot path
                             (default: ${DEFAULT_SNAPSHOT_PATH})
  --write-snapshot           write the current stable snapshot to --snapshot
                             (excludes generatedAt/samples/notes/mode/scannedFiles)
  --root, -r <path>          scan root relative to repo (repeatable).
                             default: backend
  --format, -f <text|json>   output format (default: text)
  --json                     alias for --format=json
  --help, -h                 show this help
`);
    return;
  }

  const mode = String(args.mode ?? "report");
  if (mode !== "report" && mode !== "gate") {
    throw new Error(`invalid --mode "${mode}" (expected report|gate)`);
  }

  const roots = Array.isArray(args.root) && args.root.length > 0
    ? args.root.map(String)
    : DEFAULT_ROOTS;

  const format = args.json ? "json" : String(args.format ?? "text");
  if (format !== "text" && format !== "json") {
    throw new Error(`invalid --format "${format}" (expected text|json)`);
  }

  const snapshotPath = String(args.snapshot ?? DEFAULT_SNAPSHOT_PATH);
  const writeSnapshot = Boolean(args["write-snapshot"]);

  const buckets = createBuckets();
  const files = await collectRustFiles(roots, WORKSPACE_ROOT);
  const bridgeFiles = new Set<string>();

  for (const file of files) {
    const relPath = rel(file, WORKSPACE_ROOT);
    if (isBridgePath(relPath)) bridgeFiles.add(relPath);
    const source = await Deno.readTextFile(file);
    scanFile(relPath, source, buckets);
  }

  const allowlistIssues = staticAllowlistIssues(buckets);
  const notes = mode === "gate"
    ? [
      "S10 gate: exact stable-snapshot compare + hard denylist on test_real_dirs.",
      `snapshot: ${snapshotPath}`,
    ]
    : [
      "Report mode: metrics are informational and do not fail CI.",
      `Use --mode=gate (snapshot: ${snapshotPath}) for the S10 residual budget check.`,
    ];
  const byCategory = new Map<StaticCategory, number>();
  for (const { path, name, category } of STATIC_ALLOWLIST) {
    if (buckets.allowlistedStatics.byKey.has(staticAllowlistKey(path, name))) {
      byCategory.set(category, (byCategory.get(category) ?? 0) + 1);
    }
  }
  notes.push(
    `allowlisted statics: ${buckets.allowlistedStatics.total} (${
      [...byCategory].map(([category, n]) => `${category} ${n}`).join(", ")
    }; STATIC_ALLOWLIST)`,
  );
  for (const issue of allowlistIssues) {
    notes.push(`static allowlist: ${issue}`);
  }

  const report: LedgerReport = {
    generatedAt: new Date().toISOString(),
    mode,
    roots,
    scannedFiles: files.length,
    metrics: metricsFromBuckets(buckets),
    bridgeFiles: [...bridgeFiles].sort(),
    notes,
  };

  if (writeSnapshot) {
    const stable = reportToStableSnapshot(report);
    const abs = path.isAbsolute(snapshotPath)
      ? snapshotPath
      : path.join(WORKSPACE_ROOT, snapshotPath);
    await Deno.writeTextFile(abs, `${JSON.stringify(stable, null, 2)}\n`);
    consola.success(`wrote stable snapshot: ${snapshotPath}`);
  }

  if (format === "json") {
    console.log(JSON.stringify(report, null, 2));
  } else {
    printHuman(report);
  }

  if (mode === "report") {
    Deno.exit(0);
  }

  // Gate mode
  const loaded = await loadSnapshotFile(snapshotPath);
  if (!loaded.snapshot) {
    consola.error(loaded.error ?? "failed to load snapshot");
    console.log(
      "Regenerate with: deno task architecture-ledger --write-snapshot",
    );
    Deno.exit(1);
  }

  const current = reportToStableSnapshot(report);
  const result = evaluateGate(current, loaded.snapshot, allowlistIssues);
  printGateResult(result, report, snapshotPath);
  Deno.exit(result.ok ? 0 : 1);
}

if (import.meta.main) {
  main().catch((err) => {
    consola.error(err);
    Deno.exit(1);
  });
}
