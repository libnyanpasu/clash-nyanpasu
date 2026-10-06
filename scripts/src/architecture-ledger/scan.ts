/** Rust source discovery and architecture metric scanning. */
import { walk } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
import { consola } from "../shared/logger.ts";
import {
  CONFIG_CALL_RE,
  type Hit,
  isDedicatedTestPath,
  isLegacyDtoAllowlisted,
  isRustSource,
  isStaticAllowlisted,
  isTestCfgAttr,
  LEGACY_DTO_RE,
  matchRealDirDenylist,
  type MetricBucket,
  type MetricBuckets,
  MIGRATION_MARKER_RE,
  SERVICE_GLOBAL_RE,
  STATIC_ALLOWLIST,
  STATIC_GATE_PREFIXES,
  STATIC_ITEM_RE,
  STATIC_KEYWORD_RE,
  staticAllowlistKey,
} from "./policy.ts";
import { createLexState, stripCommentsAndLiterals } from "./lexer.ts";

const SKIP_DIR_NAMES = new Set([
  ".git",
  "node_modules",
  "target",
  "dist",
  "tmp",
  ".candidates",
  // Upstream submodule under `backend/`. The ledger measures this repo's own
  // migration residual; vendored upstream sources are not ours to migrate.
  "nyanpasu-runtime",
]);

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
    mutableStatics: emptyBucket(
      "mutable_statics",
      "statics outside the allowlist",
    ),
    allowlistedStatics: emptyBucket(
      "allowlisted_statics",
      "allowlisted statics",
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

export async function collectRustFiles(
  roots: string[],
  repoRoot: string,
): Promise<string[]> {
  const files: string[] = [];
  for (const root of roots) {
    const abs = path.isAbsolute(root) ? root : path.join(repoRoot, root);
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
      const parts = path.relative(repoRoot, entry.path).split(path.SEPARATOR);
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
  const state = createLexState();
  for (let i = startLine; i < lines.length; i++) {
    const line = stripCommentsAndLiterals(lines[i], state);
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
  const state = createLexState();
  for (let i = startLine; i < lines.length; i++) {
    const line = stripCommentsAndLiterals(lines[i], state);
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

/**
 * Every `static` item in a file's comment- and literal-stripped lines: any
 * visibility, `mut`, and those declared inside `thread_local!` or
 * `lazy_static!`, including declarations split over lines. A `static` keyword
 * whose item cannot be read, such as a `macro_rules!` template, is still
 * reported, as `static@<line>`, so no declaration shape escapes the gate.
 */
export function findStatics(
  codeLines: string[],
): Array<{ name: string; line: number }> {
  const code = codeLines.join("\n");
  return matchAll(STATIC_KEYWORD_RE, code).map((m) => {
    const line = code.slice(0, m.index).split("\n").length;
    STATIC_ITEM_RE.lastIndex = m.index;
    const item = STATIC_ITEM_RE.exec(code);
    return { name: item ? item[1] : `static@${line}`, line };
  });
}

export function scanFile(
  relPath: string,
  source: string,
  buckets: MetricBuckets,
): void {
  const lines = source.split(/\r?\n/);
  const testMask = testLineMask(lines, isDedicatedTestPath(relPath));
  const countLegacyDtos = !isLegacyDtoAllowlisted(relPath);
  const lexState = createLexState();
  const codeLines: string[] = [];

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const lineNo = i + 1;
    const code = stripCommentsAndLiterals(raw, lexState);
    codeLines.push(code);

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

    if (countLegacyDtos) {
      for (const m of matchAll(LEGACY_DTO_RE, code)) {
        record(buckets.legacyDtos, m.match, {
          file: relPath,
          line: lineNo,
          text: raw.trim(),
        });
      }
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

  if (STATIC_GATE_PREFIXES.some((prefix) => relPath.startsWith(prefix))) {
    const statics = findStatics(codeLines);
    const sameName = new Map<string, number>();
    for (const { name } of statics) {
      sameName.set(name, (sameName.get(name) ?? 0) + 1);
    }
    for (const { name, line } of statics) {
      // An entry names one static. When the file declares that name twice it
      // cannot say which one it vouches for, so it covers neither.
      const listed = sameName.get(name) === 1 &&
        isStaticAllowlisted(relPath, name);
      record(
        listed ? buckets.allowlistedStatics : buckets.mutableStatics,
        staticAllowlistKey(relPath, name),
        { file: relPath, line, text: lines[line - 1].trim() },
      );
    }
  }
}

/**
 * Allowlist entries that do not name exactly one static in the scan: a static
 * that was removed or duplicated must not keep its entry.
 */
export function staticAllowlistIssues(buckets: MetricBuckets): string[] {
  return STATIC_ALLOWLIST.flatMap(({ path, name }) => {
    const key = staticAllowlistKey(path, name);
    if (buckets.allowlistedStatics.byKey.get(key) === 1) return [];
    const found = buckets.mutableStatics.byKey.get(key) ?? 0;
    return [`${key} matches ${found === 0 ? "no static" : `${found} statics`}`];
  });
}
