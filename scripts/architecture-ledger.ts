/**
 * Architecture ledger for the actor-migration residual surface.
 *
 * S01 shipped report-only mode (always exits 0 on successful scan).
 * S10 adds gate mode: committed stable-snapshot exact compare + hard
 * denylist on test real-dir hits.
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
export const DEFAULT_ROOTS = ["backend"];
export const DEFAULT_SNAPSHOT_PATH =
  "scripts/architecture-ledger.snapshot.json";

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
 * not resolve (design §8.4).
 *
 * Counts:
 * - qualified: `dirs::app_home_dir()`, `crate::utils::dirs::profiles_path()`
 * - bare imported: `use ...::app_home_dir; app_home_dir()`
 * - free runtime helpers: `runtime_config_path()`, `candidate_config_path()`
 *
 * Excludes:
 * - method receivers: `paths.app_home_dir()`, `resolver.cache_dir()`
 * - obvious function definitions: `fn app_home_dir(`, `pub async fn cache_dir(`
 */
export const REAL_DIR_HELPER_NAMES = [
  "app_config_dir",
  "app_data_dir",
  "app_home_dir",
  "app_profiles_dir",
  "app_logs_dir",
  "app_resources_dir",
  "app_install_dir",
  "nyanpasu_config_path",
  "profiles_path",
  "clash_guard_overrides_path",
  "clash_pid_path",
  "storage_path",
  "cache_dir",
  "tray_icons_path",
  "runtime_config_path",
  "candidate_config_path",
] as const;

const REAL_DIR_HELPER_ALT = REAL_DIR_HELPER_NAMES.join("|");

const REAL_DIR_DENYLIST_RE = new RegExp(
  String
    .raw`\b(?:(?:crate::)?(?:utils::)?dirs::|(?:crate::)?(?:client::)?runtime::)?(?:${REAL_DIR_HELPER_ALT})\s*\(`,
  "g",
);

const REAL_DIR_HELPER_NAME_RE = new RegExp(
  String.raw`(${REAL_DIR_HELPER_ALT})\s*\($`,
);

export type Hit = {
  file: string;
  line: number;
  text: string;
  detail?: string;
};

export type MetricBucket = {
  id: string;
  label: string;
  total: number;
  byKey: Map<string, number>;
  hits: Hit[];
};

export type MetricBuckets = {
  configCalls: MetricBucket;
  serviceGlobals: MetricBucket;
  migrationMarkers: MetricBucket;
  legacyDtos: MetricBucket;
  testRealDirs: MetricBucket;
};

export type ReportMetric = {
  total: number;
  byKey: Record<string, number>;
  samples: Hit[];
};

export type LedgerReport = {
  generatedAt: string;
  mode: "report" | "gate";
  roots: string[];
  scannedFiles: number;
  metrics: Record<string, ReportMetric>;
  bridgeFiles: string[];
  notes: string[];
};

/** Committed residual budget: excludes nondeterministic / report-only fields. */
export type StableMetric = {
  total: number;
  byKey: Record<string, number>;
};

export type StableSnapshot = {
  roots: string[];
  metrics: Record<string, StableMetric>;
  bridgeFiles: string[];
};

export type GateIssue = {
  kind:
    | "hard_denylist"
    | "metric_total"
    | "metric_key"
    | "roots"
    | "bridge_files"
    | "snapshot";
  message: string;
};

export type GateResult = {
  ok: boolean;
  issues: GateIssue[];
};

export function rel(filePath: string, root = ROOT): string {
  return path.relative(root, filePath).split(path.SEPARATOR).join("/");
}

export function isRustSource(filePath: string): boolean {
  return filePath.endsWith(".rs");
}

export function isBridgePath(relPath: string): boolean {
  return (
    relPath.includes("/bridge/") ||
    /(^|\/)[^/]*bridge[^/]*\.rs$/i.test(relPath)
  );
}

export function isDedicatedTestPath(relPath: string): boolean {
  return (
    relPath.includes("/tests/") ||
    /(^|\/)[^/]+_test\.rs$/.test(relPath) ||
    // sibling module files: test.rs / tests.rs
    /(^|\/)tests?\.rs$/.test(relPath)
  );
}

/**
 * True when a `#[cfg(...)]` attribute enables the test configuration.
 * Accepts `cfg(test)`, `cfg(all(test, ...))`, `cfg(any(..., test, ...))`.
 * Rejects `cfg(not(test))` and other configs after stripping `not(...)`.
 */
export function isTestCfgAttr(trimmed: string): boolean {
  if (!/^#\[cfg\s*\(/.test(trimmed)) return false;
  const open = trimmed.indexOf("(");
  const close = trimmed.lastIndexOf(")");
  if (open < 0 || close <= open) return false;
  const inner = trimmed.slice(open + 1, close);
  return cfgInnerEnablesTest(inner);
}

/** Strip string literals and balanced `not(...)` groups, then look for bare `test`. */
export function cfgInnerEnablesTest(inner: string): boolean {
  // Drop string/char literals so `feature = "test"` is not a false positive.
  let s = inner
    .replace(/"(?:\\.|[^"\\])*"/g, '""')
    .replace(/'(?:\\.|[^'\\])*'/g, "''");
  let prev = "";
  while (s !== prev) {
    prev = s;
    s = s.replace(/not\s*\((?:[^()]|\([^()]*\))*\)/g, "");
  }
  return /(?:^|[^A-Za-z0-9_])test(?:[^A-Za-z0-9_]|$)/.test(s);
}

/**
 * True when `helperName` at `nameIndex` is an obvious `fn` definition site.
 * Keeps denylist focused on call sites (including bare imported calls).
 */
export function isLikelyFnDefinition(
  code: string,
  nameIndex: number,
): boolean {
  const before = code.slice(Math.max(0, nameIndex - 96), nameIndex);
  return /(?:^|[\s;{}])(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?fn\s+$/
    .test(before);
}

/**
 * Match denylist real-dir / runtime-path call sites in a code line.
 * Excludes method receivers and obvious function definitions.
 */
export function matchRealDirDenylist(
  code: string,
): Array<{ index: number; match: string; key: string }> {
  const out: Array<{ index: number; match: string; key: string }> = [];
  REAL_DIR_DENYLIST_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = REAL_DIR_DENYLIST_RE.exec(code)) !== null) {
    const full = m[0];
    const nameMatch = full.match(REAL_DIR_HELPER_NAME_RE);
    if (!nameMatch) continue;
    const helperName = nameMatch[1];
    const nameOffset = full.lastIndexOf(helperName);
    const nameIndex = m.index + nameOffset;
    // Method call: `paths.app_home_dir()`
    if (nameIndex > 0 && code[nameIndex - 1] === ".") continue;
    // Definition: `fn app_home_dir(` / `pub fn cache_dir(`
    if (isLikelyFnDefinition(code, nameIndex)) continue;
    // Normalize trailing `foo(` → `foo()` so keys stay call-shaped.
    const key = full.replace(/\s+/g, "").replace(/\($/, "()");
    out.push({
      index: m.index,
      match: full,
      key,
    });
  }
  return out;
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

export function emptyBucket(id: string, label: string): MetricBucket {
  return { id, label, total: 0, byKey: new Map(), hits: [] };
}

export function createBuckets(): MetricBuckets {
  return {
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
export function testLineMask(
  lines: string[],
  dedicatedTestFile: boolean,
): boolean[] {
  const mask = Array.from({ length: lines.length }, () => dedicatedTestFile);
  if (dedicatedTestFile) return mask;

  const isAttr = (trimmed: string) => trimmed.startsWith("#[");
  const isTestCfg = (trimmed: string) => isTestCfgAttr(trimmed);
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

export function scanFile(
  relPath: string,
  source: string,
  buckets: MetricBuckets,
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
      for (const m of matchRealDirDenylist(code)) {
        record(buckets.testRealDirs, m.key, {
          file: relPath,
          line: lineNo,
          text: raw.trim(),
        });
      }
    }
  }
}

export function sortedRecord(map: Map<string, number>): Record<string, number> {
  return Object.fromEntries(
    [...map.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])),
  );
}

/** Stable object-key order for exact snapshot JSON / compare. */
export function sortedObjectKeys(
  record: Record<string, number>,
): Record<string, number> {
  return Object.fromEntries(
    Object.entries(record).sort((a, b) =>
      b[1] - a[1] || a[0].localeCompare(b[0])
    ),
  );
}

export function metricsFromBuckets(
  buckets: MetricBuckets,
): Record<string, ReportMetric> {
  return {
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
  };
}

export function toStableSnapshot(
  roots: string[],
  metrics: Record<string, ReportMetric | StableMetric>,
  bridgeFiles: string[],
): StableSnapshot {
  const stableMetrics: Record<string, StableMetric> = {};
  for (const [id, metric] of Object.entries(metrics)) {
    stableMetrics[id] = {
      total: metric.total,
      byKey: sortedObjectKeys({ ...metric.byKey }),
    };
  }
  return {
    roots: [...roots],
    metrics: stableMetrics,
    bridgeFiles: [...bridgeFiles].sort(),
  };
}

export function reportToStableSnapshot(report: LedgerReport): StableSnapshot {
  return toStableSnapshot(report.roots, report.metrics, report.bridgeFiles);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Validate and normalize a JSON-decoded snapshot into the stable contract.
 * Throws with an actionable message on structural invalidity.
 */
export function parseStableSnapshot(raw: unknown): StableSnapshot {
  if (!isPlainObject(raw)) {
    throw new Error("snapshot must be a JSON object");
  }
  if (
    !Array.isArray(raw.roots) || !raw.roots.every((r) => typeof r === "string")
  ) {
    throw new Error("snapshot.roots must be string[]");
  }
  if (
    !Array.isArray(raw.bridgeFiles) ||
    !raw.bridgeFiles.every((r) => typeof r === "string")
  ) {
    throw new Error("snapshot.bridgeFiles must be string[]");
  }
  if (!isPlainObject(raw.metrics)) {
    throw new Error("snapshot.metrics must be an object");
  }

  const metrics: Record<string, StableMetric> = {};
  for (const [id, metric] of Object.entries(raw.metrics)) {
    if (!isPlainObject(metric)) {
      throw new Error(`snapshot.metrics.${id} must be an object`);
    }
    if (typeof metric.total !== "number" || !Number.isFinite(metric.total)) {
      throw new Error(`snapshot.metrics.${id}.total must be a finite number`);
    }
    if (!isPlainObject(metric.byKey)) {
      throw new Error(`snapshot.metrics.${id}.byKey must be an object`);
    }
    const byKey: Record<string, number> = {};
    for (const [key, count] of Object.entries(metric.byKey)) {
      if (typeof count !== "number" || !Number.isFinite(count)) {
        throw new Error(
          `snapshot.metrics.${id}.byKey[${
            JSON.stringify(key)
          }] must be a finite number`,
        );
      }
      byKey[key] = count;
    }
    metrics[id] = {
      total: metric.total,
      byKey: sortedObjectKeys(byKey),
    };
  }

  return {
    roots: raw.roots.map(String),
    metrics,
    bridgeFiles: [...raw.bridgeFiles.map(String)].sort(),
  };
}

function formatListDiff(
  label: string,
  expected: string[],
  actual: string[],
): string[] {
  const messages: string[] = [];
  const expSet = new Set(expected);
  const actSet = new Set(actual);
  for (const item of expected) {
    if (!actSet.has(item)) {
      messages.push(`${label}: missing ${JSON.stringify(item)}`);
    }
  }
  for (const item of actual) {
    if (!expSet.has(item)) {
      messages.push(`${label}: unexpected ${JSON.stringify(item)}`);
    }
  }
  if (
    messages.length === 0 &&
    (expected.length !== actual.length ||
      expected.some((v, i) => v !== actual[i]))
  ) {
    messages.push(
      `${label}: order/content mismatch expected=${
        JSON.stringify(expected)
      } actual=${JSON.stringify(actual)}`,
    );
  }
  return messages;
}

/**
 * Exact stable-snapshot compare + hard denylist on test_real_dirs.
 * Intentional residual shrink/growth requires an audited snapshot update.
 */
export function evaluateGate(
  current: StableSnapshot,
  expected: StableSnapshot,
): GateResult {
  const issues: GateIssue[] = [];

  const denylistTotal = current.metrics.test_real_dirs?.total ?? 0;
  if (denylistTotal !== 0) {
    issues.push({
      kind: "hard_denylist",
      message:
        `hard denylist: test_real_dirs.total must be 0, got ${denylistTotal}`,
    });
  }

  for (
    const msg of formatListDiff("roots", expected.roots, current.roots)
  ) {
    issues.push({ kind: "roots", message: msg });
  }

  for (
    const msg of formatListDiff(
      "bridgeFiles",
      expected.bridgeFiles,
      current.bridgeFiles,
    )
  ) {
    issues.push({ kind: "bridge_files", message: msg });
  }

  const expectedIds = new Set(Object.keys(expected.metrics));
  const currentIds = new Set(Object.keys(current.metrics));

  for (const id of expectedIds) {
    if (!currentIds.has(id)) {
      issues.push({
        kind: "metric_key",
        message: `metrics: missing metric id ${JSON.stringify(id)}`,
      });
    }
  }
  for (const id of currentIds) {
    if (!expectedIds.has(id)) {
      issues.push({
        kind: "metric_key",
        message: `metrics: unexpected metric id ${JSON.stringify(id)}`,
      });
    }
  }

  for (const id of expectedIds) {
    if (!currentIds.has(id)) continue;
    const exp = expected.metrics[id];
    const act = current.metrics[id];
    if (exp.total !== act.total) {
      issues.push({
        kind: "metric_total",
        message:
          `metrics.${id}.total: expected ${exp.total}, actual ${act.total}`,
      });
    }

    const expKeys = new Set(Object.keys(exp.byKey));
    const actKeys = new Set(Object.keys(act.byKey));
    for (const key of expKeys) {
      if (!actKeys.has(key)) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey: missing key ${
            JSON.stringify(key)
          } (expected ${exp.byKey[key]})`,
        });
        continue;
      }
      if (exp.byKey[key] !== act.byKey[key]) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey[${JSON.stringify(key)}]: expected ${
            exp.byKey[key]
          }, actual ${act.byKey[key]}`,
        });
      }
    }
    for (const key of actKeys) {
      if (!expKeys.has(key)) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey: unexpected key ${
            JSON.stringify(key)
          } (actual ${act.byKey[key]})`,
        });
      }
    }
  }

  return { ok: issues.length === 0, issues };
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

function printGateResult(
  result: GateResult,
  report: LedgerReport,
  snapshotPath: string,
): void {
  console.log("");
  if (result.ok) {
    consola.success(
      `architecture ledger gate passed (snapshot: ${snapshotPath})`,
    );
    return;
  }

  consola.error(
    `architecture ledger gate FAILED (${result.issues.length} issue(s); snapshot: ${snapshotPath})`,
  );
  for (const issue of result.issues) {
    console.log(`  [${issue.kind}] ${issue.message}`);
  }

  const denylist = report.metrics.test_real_dirs;
  if (denylist && denylist.total > 0 && denylist.samples.length > 0) {
    console.log("  denylist samples:");
    for (const sample of denylist.samples.slice(0, 12)) {
      console.log(
        `    ${sample.file}:${sample.line}: ${sample.detail ?? sample.text}`,
      );
    }
  }

  console.log("");
  console.log(
    "To accept intentional residual changes after review, regenerate the committed snapshot:",
  );
  console.log(
    `  deno run -A scripts/architecture-ledger.ts --write-snapshot --snapshot ${snapshotPath}`,
  );
}

async function loadSnapshotFile(
  snapshotPath: string,
): Promise<{ snapshot?: StableSnapshot; error?: string }> {
  const abs = path.isAbsolute(snapshotPath)
    ? snapshotPath
    : path.join(ROOT, snapshotPath);
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
    console.log(`Usage: deno run -A scripts/architecture-ledger.ts [options]

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
  const files = await collectRustFiles(roots);
  const bridgeFiles = new Set<string>();

  for (const file of files) {
    const relPath = rel(file);
    if (isBridgePath(relPath)) bridgeFiles.add(relPath);
    const source = await Deno.readTextFile(file);
    scanFile(relPath, source, buckets);
  }

  const notes = mode === "gate"
    ? [
      "S10 gate: exact stable-snapshot compare + hard denylist on test_real_dirs.",
      `snapshot: ${snapshotPath}`,
    ]
    : [
      "Report mode: metrics are informational and do not fail CI.",
      `Use --mode=gate (snapshot: ${snapshotPath}) for the S10 residual budget check.`,
    ];

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
      : path.join(ROOT, snapshotPath);
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
      "Regenerate with: deno run -A scripts/architecture-ledger.ts --write-snapshot",
    );
    Deno.exit(1);
  }

  const current = reportToStableSnapshot(report);
  const result = evaluateGate(current, loaded.snapshot);
  printGateResult(result, report, snapshotPath);
  Deno.exit(result.ok ? 0 : 1);
}

if (import.meta.main) {
  main().catch((err) => {
    consola.error(err);
    Deno.exit(1);
  });
}
