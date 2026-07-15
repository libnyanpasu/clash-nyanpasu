/**
 * Architecture ledger for the actor-migration residual surface.
 *
 * S01 ships report-only mode (always exits 0 on successful scan).
 * Gate-mode thresholds and committed-snapshot diff land in S10.
 *
 * Metrics:
 * - Config::*() call sites
 * - service ::global() call sites
 * - TODO/FIXME(actor-migration) markers
 * - bridge files + legacy DTO references
 * - test real-dir / runtime-path denylist hits
 */
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { walk } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
import { consola } from "./utils/logger.ts";

const ROOT = Deno.cwd();
const DEFAULT_ROOTS = ["backend"];

const SKIP_DIR_NAMES = new Set([
  ".git",
  "node_modules",
  "target",
  "dist",
  "tmp",
  ".candidates",
]);

/** Config facade call sites still bound to the legacy global config graph. */
const CONFIG_CALL_RE = /\bConfig::([A-Za-z_][A-Za-z0-9_]*)\s*\(/g;

/** Service-style `Foo::global()` lookups (excludes bare `global(`). */
const SERVICE_GLOBAL_RE = /\b([A-Za-z_][A-Za-z0-9_]*)::global\s*\(/g;

/** Migration residual markers required by CLAUDE.md §15. */
const MIGRATION_MARKER_RE = /\b(?:TODO|FIXME)\s*\(\s*actor-migration\s*\)/g;

/**
 * Legacy DTO / bridge type surface that should shrink toward zero by PR-7.
 * Keep the list explicit so unrelated "legacy" prose is not counted.
 */
const LEGACY_DTO_RE =
  /\b(?:IVerge|IClashTemp|IClash|IProfiles|ProfilesBuilder|LegacyVergeBridge|LegacyClashBridge|LegacyWindowBridge|VergeLegacyBridge|ClashLegacyBridge|legacy_iverge_from_typed|legacy_iverge_base_for_typed_read|typed_config_from_legacy|typed_patches_from_legacy_patch|LegacyVergePatchRoute)\b/g;

/**
 * Real product/user-dir and free global runtime path helpers that tests must
 * not resolve (design §8.4). Require an explicit `dirs::` / `utils::dirs::`
 * qualifier so injected `PathResolver` methods (`paths.profiles_path()` etc.)
 * are not counted. Free global runtime helpers are matched by name.
 */
const REAL_DIR_DENYLIST_RE =
  /\b(?:crate::)?(?:utils::)?dirs::(?:app_config_dir|app_data_dir|app_home_dir|app_profiles_dir|app_logs_dir|app_resources_dir|app_install_dir|nyanpasu_config_path|profiles_path|clash_guard_overrides_path|clash_pid_path|storage_path)\s*\(|\b(?:crate::client::runtime::)?(?:runtime_config_path|candidate_config_path)\s*\(/g;

type Hit = {
  file: string;
  line: number;
  text: string;
  detail?: string;
};

type MetricBucket = {
  id: string;
  label: string;
  total: number;
  byKey: Map<string, number>;
  hits: Hit[];
};

type LedgerReport = {
  generatedAt: string;
  mode: "report" | "gate";
  roots: string[];
  scannedFiles: number;
  metrics: Record<
    string,
    {
      total: number;
      byKey: Record<string, number>;
      samples: Hit[];
    }
  >;
  bridgeFiles: string[];
  notes: string[];
};

function rel(filePath: string): string {
  return path.relative(ROOT, filePath).split(path.SEPARATOR).join("/");
}

function isRustSource(filePath: string): boolean {
  return filePath.endsWith(".rs");
}

function isBridgePath(relPath: string): boolean {
  return (
    relPath.includes("/bridge/") ||
    /(^|\/)[^/]*bridge[^/]*\.rs$/i.test(relPath)
  );
}

function isDedicatedTestPath(relPath: string): boolean {
  return (
    relPath.includes("/tests/") ||
    /(^|\/)[^/]+_test\.rs$/.test(relPath) ||
    /(^|\/)test\.rs$/.test(relPath)
  );
}

function stripLineComment(line: string): string {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    const prev = i > 0 ? line[i - 1] : "";
    if (ch === "'" && !inDouble && prev !== "\\") inSingle = !inSingle;
    if (ch === '"' && !inSingle && prev !== "\\") inDouble = !inDouble;
    if (
      !inSingle &&
      !inDouble &&
      ch === "/" &&
      i + 1 < line.length &&
      line[i + 1] === "/"
    ) {
      return line.slice(0, i);
    }
  }
  return line;
}

function emptyBucket(id: string, label: string): MetricBucket {
  return { id, label, total: 0, byKey: new Map(), hits: [] };
}

function record(
  bucket: MetricBucket,
  key: string,
  hit: Hit,
  sampleLimit = 20,
): void {
  bucket.total += 1;
  bucket.byKey.set(key, (bucket.byKey.get(key) ?? 0) + 1);
  if (bucket.hits.length < sampleLimit) {
    bucket.hits.push({ ...hit, detail: key });
  }
}

function matchAll(
  re: RegExp,
  text: string,
): Array<{ index: number; match: string; groups: string[] }> {
  const out: Array<{ index: number; match: string; groups: string[] }> = [];
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    out.push({
      index: m.index,
      match: m[0],
      groups: m.slice(1),
    });
    if (m[0].length === 0) re.lastIndex += 1;
  }
  return out;
}

async function collectRustFiles(roots: string[]): Promise<string[]> {
  const files: string[] = [];
  for (const root of roots) {
    const abs = path.isAbsolute(root) ? root : path.join(ROOT, root);
    try {
      const st = await Deno.stat(abs);
      if (!st.isDirectory) continue;
    } catch {
      consola.warn(`skip missing root: ${root}`);
      continue;
    }

    for await (
      const entry of walk(abs, {
        includeDirs: false,
        includeFiles: true,
        exts: ["rs"],
        skip: [/[/\\]target[/\\]/, /[/\\]node_modules[/\\]/],
      })
    ) {
      const parts = entry.path.split(path.SEPARATOR);
      if (parts.some((p) => SKIP_DIR_NAMES.has(p))) continue;
      if (!isRustSource(entry.path)) continue;
      files.push(entry.path);
    }
  }
  files.sort();
  return files;
}

/**
 * Approximate test-region detection:
 * - whole file if dedicated test path
 * - bodies of `#[cfg(test)] mod ... { ... }` / trailing `mod tests`
 * - single items tagged with `#[cfg(test)]` or `#[test]` / `#[tokio::test]`
 *
 * Early `#[cfg(test)] pub use ...` must NOT mark the rest of the file.
 */
function testLineMask(lines: string[], dedicatedTestFile: boolean): boolean[] {
  const mask = Array.from({ length: lines.length }, () => dedicatedTestFile);
  if (dedicatedTestFile) return mask;

  const isAttr = (trimmed: string) => trimmed.startsWith("#[");
  const isTestCfg = (trimmed: string) =>
    /^#\[cfg\s*\(\s*test\s*\)\]/.test(trimmed);
  const isTestFnAttr = (trimmed: string) =>
    /^#\[(?:tokio::)?test(?:\s*\(.*\))?\]/.test(trimmed);

  let i = 0;
  while (i < lines.length) {
    const trimmed = lines[i].trim();
    if (!isTestCfg(trimmed) && !isTestFnAttr(trimmed)) {
      i += 1;
      continue;
    }

    // Consume contiguous attributes belonging to the same item.
    let j = i;
    let sawTestFn = false;
    let sawCfgTest = false;
    while (j < lines.length) {
      const t = lines[j].trim();
      if (!isAttr(t)) break;
      if (isTestCfg(t)) sawCfgTest = true;
      if (isTestFnAttr(t)) sawTestFn = true;
      j += 1;
    }
    if (j >= lines.length) break;

    const item = lines[j].trim();
    // `mod tests {` / `mod foo {` under cfg(test) — mark the whole module.
    if (sawCfgTest && /^mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{/.test(item)) {
      const end = findMatchingBraceLine(lines, j);
      for (let k = i; k <= end; k++) mask[k] = true;
      i = end + 1;
      continue;
    }

    // `#[test] fn ...` or `#[cfg(test)] fn/use/const/...` — mark one item.
    if (sawTestFn || sawCfgTest) {
      const end = findItemEndLine(lines, j);
      for (let k = i; k <= end; k++) mask[k] = true;
      i = end + 1;
      continue;
    }

    i = j + 1;
  }

  return mask;
}

function findMatchingBraceLine(lines: string[], startLine: number): number {
  let depth = 0;
  let seen = false;
  for (let i = startLine; i < lines.length; i++) {
    const line = stripLineComment(lines[i]);
    for (const ch of line) {
      if (ch === "{") {
        depth += 1;
        seen = true;
      } else if (ch === "}") {
        depth -= 1;
        if (seen && depth === 0) return i;
      }
    }
  }
  return lines.length - 1;
}

/** End line of a single Rust item starting at `startLine` (brace-aware). */
function findItemEndLine(lines: string[], startLine: number): number {
  // Items without a body end at the first `;`.
  // Items with `{ ... }` end at the matching brace.
  let depth = 0;
  let seenBrace = false;
  for (let i = startLine; i < lines.length; i++) {
    const line = stripLineComment(lines[i]);
    for (const ch of line) {
      if (ch === "{") {
        depth += 1;
        seenBrace = true;
      } else if (ch === "}") {
        depth -= 1;
        if (seenBrace && depth === 0) return i;
      } else if (ch === ";" && !seenBrace && depth === 0) {
        return i;
      }
    }
  }
  return lines.length - 1;
}

function scanFile(
  relPath: string,
  source: string,
  buckets: {
    configCalls: MetricBucket;
    serviceGlobals: MetricBucket;
    migrationMarkers: MetricBucket;
    legacyDtos: MetricBucket;
    testRealDirs: MetricBucket;
  },
): void {
  const lines = source.split(/\r?\n/);
  const testMask = testLineMask(lines, isDedicatedTestPath(relPath));

  // Block-comment strip is best-effort; migration TODOs live in // comments
  // and must remain visible, so only line-level // stripping is applied for
  // code-like metrics, while migration markers scan the raw line.
  let inBlockComment = false;

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const lineNo = i + 1;

    // Track /* */ so code metrics ignore commented-out call sites.
    let code = "";
    for (let j = 0; j < raw.length; j++) {
      if (!inBlockComment && raw[j] === "/" && raw[j + 1] === "*") {
        inBlockComment = true;
        j++;
        continue;
      }
      if (inBlockComment) {
        if (raw[j] === "*" && raw[j + 1] === "/") {
          inBlockComment = false;
          j++;
        }
        continue;
      }
      code += raw[j];
    }
    code = stripLineComment(code);

    for (const m of matchAll(CONFIG_CALL_RE, code)) {
      record(buckets.configCalls, `Config::${m.groups[0]}()`, {
        file: relPath,
        line: lineNo,
        text: raw.trim(),
      });
    }

    for (const m of matchAll(SERVICE_GLOBAL_RE, code)) {
      // Config::global is already represented under Config::* metrics; still
      // count it here so the service-global residual is complete.
      record(buckets.serviceGlobals, `${m.groups[0]}::global()`, {
        file: relPath,
        line: lineNo,
        text: raw.trim(),
      });
    }

    for (const m of matchAll(MIGRATION_MARKER_RE, raw)) {
      const kind = m.match.startsWith("FIXME") ? "FIXME" : "TODO";
      record(buckets.migrationMarkers, `${kind}(actor-migration)`, {
        file: relPath,
        line: lineNo,
        text: raw.trim(),
      });
    }

    for (const m of matchAll(LEGACY_DTO_RE, code)) {
      record(buckets.legacyDtos, m.match, {
        file: relPath,
        line: lineNo,
        text: raw.trim(),
      });
    }

    if (testMask[i]) {
      for (const m of matchAll(REAL_DIR_DENYLIST_RE, code)) {
        const key = m.match.replace(/\s+/g, "");
        record(buckets.testRealDirs, key, {
          file: relPath,
          line: lineNo,
          text: raw.trim(),
        });
      }
    }
  }
}

function sortedRecord(map: Map<string, number>): Record<string, number> {
  return Object.fromEntries(
    [...map.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])),
  );
}

function printHuman(report: LedgerReport): void {
  consola.info("architecture ledger (actor-migration residuals)");
  consola.info(`mode: ${report.mode}`);
  consola.info(`roots: ${report.roots.join(", ")}`);
  consola.info(`scanned rust files: ${report.scannedFiles}`);
  console.log("");

  const rows: Array<[string, number]> = Object.entries(report.metrics).map(
    ([id, metric]) => [id, metric.total],
  );
  const labelWidth = Math.max(...rows.map(([id]) => id.length), 8);
  for (const [id, total] of rows) {
    console.log(`${id.padEnd(labelWidth)}  ${String(total).padStart(6)}`);
  }

  console.log("");
  console.log(
    `bridge_files                 ${
      String(report.bridgeFiles.length).padStart(6)
    }`,
  );
  if (report.bridgeFiles.length > 0) {
    for (const file of report.bridgeFiles) {
      console.log(`  - ${file}`);
    }
  }

  for (const [id, metric] of Object.entries(report.metrics)) {
    const keys = Object.entries(metric.byKey);
    if (keys.length === 0) continue;
    console.log("");
    console.log(`[${id}] breakdown`);
    for (const [key, count] of keys.slice(0, 25)) {
      console.log(`  ${String(count).padStart(5)}  ${key}`);
    }
    if (keys.length > 25) {
      console.log(`  ... ${keys.length - 25} more keys`);
    }
    if (metric.samples.length > 0) {
      console.log(`  samples (up to ${metric.samples.length}):`);
      for (const sample of metric.samples.slice(0, 8)) {
        console.log(
          `    ${sample.file}:${sample.line}: ${sample.detail ?? ""}`,
        );
      }
    }
  }

  if (report.notes.length > 0) {
    console.log("");
    for (const note of report.notes) {
      consola.info(note);
    }
  }
}

async function main(): Promise<void> {
  const args = parseArgs(Deno.args, {
    string: ["mode", "root", "format"],
    collect: ["root"],
    default: {
      mode: "report",
      format: "text",
    },
    alias: {
      m: "mode",
      r: "root",
      f: "format",
      h: "help",
    },
    boolean: ["help", "json"],
  });

  if (args.help) {
    console.log(`Usage: deno run -A scripts/architecture-ledger.ts [options]

Options:
  --mode, -m <report|gate>   report (default) always exits 0 after printing.
                             gate is reserved for S10 CI thresholds; currently
                             still report-only and exits 0.
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

  const buckets = {
    configCalls: emptyBucket("config_calls", "Config::*() call sites"),
    serviceGlobals: emptyBucket(
      "service_globals",
      "service ::global() call sites",
    ),
    migrationMarkers: emptyBucket(
      "migration_markers",
      "TODO/FIXME(actor-migration)",
    ),
    legacyDtos: emptyBucket(
      "legacy_dto_refs",
      "bridge / legacy DTO references",
    ),
    testRealDirs: emptyBucket(
      "test_real_dirs",
      "test real-dir / runtime-path hits",
    ),
  };

  const files = await collectRustFiles(roots);
  const bridgeFiles = new Set<string>();

  for (const file of files) {
    const relPath = rel(file);
    if (isBridgePath(relPath)) bridgeFiles.add(relPath);
    const source = await Deno.readTextFile(file);
    scanFile(relPath, source, buckets);
  }

  const notes = [
    "S01 report-only: metrics are informational and do not fail CI.",
    mode === "gate"
      ? "Gate thresholds / snapshot diff are reserved for S10; this run still exits 0."
      : "Use --mode=gate later (S10) once residual budgets are committed.",
  ];

  const report: LedgerReport = {
    generatedAt: new Date().toISOString(),
    mode,
    roots,
    scannedFiles: files.length,
    metrics: {
      config_calls: {
        total: buckets.configCalls.total,
        byKey: sortedRecord(buckets.configCalls.byKey),
        samples: buckets.configCalls.hits,
      },
      service_globals: {
        total: buckets.serviceGlobals.total,
        byKey: sortedRecord(buckets.serviceGlobals.byKey),
        samples: buckets.serviceGlobals.hits,
      },
      migration_markers: {
        total: buckets.migrationMarkers.total,
        byKey: sortedRecord(buckets.migrationMarkers.byKey),
        samples: buckets.migrationMarkers.hits,
      },
      legacy_dto_refs: {
        total: buckets.legacyDtos.total,
        byKey: sortedRecord(buckets.legacyDtos.byKey),
        samples: buckets.legacyDtos.hits,
      },
      test_real_dirs: {
        total: buckets.testRealDirs.total,
        byKey: sortedRecord(buckets.testRealDirs.byKey),
        samples: buckets.testRealDirs.hits,
      },
    },
    bridgeFiles: [...bridgeFiles].sort(),
    notes,
  };

  if (format === "json") {
    console.log(JSON.stringify(report, null, 2));
  } else {
    printHuman(report);
  }

  // Report-only (and pre-S10 gate placeholder): successful scan always exits 0.
  Deno.exit(0);
}

main().catch((err) => {
  consola.error(err);
  Deno.exit(1);
});
