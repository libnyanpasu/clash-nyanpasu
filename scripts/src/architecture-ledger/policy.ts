/** Architecture ledger policy, classifications, and shared report types. */
import * as path from "jsr:@std/path";

export const DEFAULT_ROOTS = ["backend"];
export const DEFAULT_SNAPSHOT_PATH =
  "scripts/fixtures/architecture-ledger.snapshot.json";

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

/**
 * Config facade call sites still bound to the legacy global config graph.
 * A path-qualified `Config` (`meta::Config::new()`) is another crate's type.
 */
export const CONFIG_CALL_RE = /(?<!::)\bConfig::([A-Za-z_][A-Za-z0-9_]*)\s*\(/g;

/** Service-style `Foo::global()` lookups (excludes bare `global(`). */
export const SERVICE_GLOBAL_RE = /\b([A-Za-z_][A-Za-z0-9_]*)::global\s*\(/g;

/** Migration residual markers required by CLAUDE.md §15. */
export const MIGRATION_MARKER_RE =
  /\b(?:TODO|FIXME)\s*\(\s*actor-migration\s*\)/g;

/**
 * Legacy DTO / bridge type surface that should shrink toward zero by PR-7.
 * Keep the list explicit so unrelated "legacy" prose is not counted.
 */
export const LEGACY_DTO_RE =
  /\b(?:IVerge|IClashTemp|IClash|IProfiles|ProfilesBuilder|LegacyVergeBridge|LegacyClashBridge|LegacyWindowBridge|VergeLegacyBridge|ClashLegacyBridge|legacy_iverge_from_typed|legacy_iverge_base_for_typed_read|typed_config_from_legacy|typed_patches_from_legacy_patch|LegacyVergePatchRoute)\b/g;

/**
 * Paths excluded from `legacy_dto_refs`. Each entry must say why its legacy
 * names are not an application DTO surface. A path ending in `/` covers that
 * directory; any other path covers exactly that file.
 */
export const LEGACY_DTO_ALLOWLIST: ReadonlyArray<
  { paths: ReadonlyArray<string>; reason: string }
> = [
  {
    paths: [
      "backend/tauri/src/core/migration/legacy_schema/",
      "backend/tauri/src/core/migration/modules/typed_config.rs",
    ],
    // The pre-typed `verge.yaml` / clash overrides shape the typed config
    // migration reads to upgrade old installs, and the migration module that
    // reads it; nothing else may use it.
    reason: "on-disk upgrade input schema, not an application DTO",
  },
];

export function isLegacyDtoAllowlisted(relPath: string): boolean {
  return LEGACY_DTO_ALLOWLIST.some(({ paths }) =>
    paths.some((entry) =>
      entry.endsWith("/") ? relPath.startsWith(entry) : relPath === entry
    )
  );
}

/** A `static` keyword: not a `'static` lifetime, not part of an identifier. */
export const STATIC_KEYWORD_RE = /(?<![\w'#$])static(?!\w)/g;

/**
 * The rest of a `static` item from its keyword: `mut`, or `ref` as
 * `lazy_static!` writes it, then the name up to the colon before its type.
 */
export const STATIC_ITEM_RE =
  /static\s+(?:(?:mut|ref)\s+)?((?:r#)?[A-Za-z_][A-Za-z0-9_]*)\s*:/y;

/** Only the app crate is gated; the other backend crates are libraries. */
export const STATIC_GATE_PREFIX = "backend/tauri/src/";

/**
 * Why a static may exist (docs/development/architecture.md):
 * - `immutable`: a constant or lookup table, initialized at most once and
 *   never written afterwards;
 * - `external`: a global that an OS or third-party API imposes;
 * - `test`: compiled only into tests.
 *
 * Mutable service state fits none of them, so it has no way into the list.
 */
export type StaticCategory = "immutable" | "external" | "test";

export type StaticAllowlistEntry = {
  path: string;
  name: string;
  category: StaticCategory;
  reason: string;
};

/** The statics of one config migration module; they are all stateless. */
function migrationModuleStatics(
  module: string,
  steps: string[],
): StaticAllowlistEntry[] {
  const path = `backend/tauri/src/core/migration/modules/${module}.rs`;
  return [
    {
      path,
      name: "MIGRATOR",
      category: "immutable",
      reason: "stateless module migrator the registry points at",
    },
    {
      path,
      name: "VERSION_2_0_0",
      category: "immutable",
      reason: "constant version of the module's steps",
    },
    {
      path,
      name: "STEPS",
      category: "immutable",
      reason: "lookup table of the module's steps",
    },
    ...steps.map((name): StaticAllowlistEntry => ({
      path,
      name,
      category: "immutable",
      reason: "stateless migration step",
    })),
  ];
}

/**
 * Every static the app crate may declare. The type of a static cannot show
 * that it is immutable (an alias, a newtype or a wrapper hides it), so every
 * non-`const` static counts against the gate until a reviewer lists it here
 * with its category and reason.
 */
export const STATIC_ALLOWLIST: ReadonlyArray<StaticAllowlistEntry> = [
  // -- immutable constants and lookup tables --------------------------------
  {
    path: "backend/tauri/src/consts.rs",
    name: "BUILD_INFO",
    category: "immutable",
    reason: "build metadata from compile-time env vars",
  },
  {
    path: "backend/tauri/src/consts.rs",
    name: "IS_APPIMAGE",
    category: "immutable",
    reason: "launch-environment flag read once",
  },
  {
    path: "backend/tauri/src/utils/dirs.rs",
    name: "APP_VERSION",
    category: "immutable",
    reason: "version string from a compile-time env var",
  },
  {
    path: "backend/tauri/src/utils/hwid.rs",
    name: "DEVICE_INFO",
    category: "immutable",
    reason: "host identity computed once",
  },
  {
    path: "backend/tauri/src/core/migration/registry.rs",
    name: "MODULES",
    category: "immutable",
    reason: "lookup table of the migration modules",
  },
  ...migrationModuleStatics("app_config", [
    "LANGUAGE_OPTION",
    "THEME_SETTING",
    "NET_STAT_WIDGET_FLATTEN",
    "LANGUAGE_CASE",
  ]),
  ...migrationModuleStatics("profiles", [
    "NULL_VALUE",
    "SCRIPT_NEWTYPE",
    "CLEAN_SCHEMA",
    "REPAIR_SCHEMA",
  ]),
  ...migrationModuleStatics("storage", [
    "HOTKEYS_TO_KV",
    "HOTKEYS_TO_TYPED_CONFIG",
  ]),
  ...migrationModuleStatics("typed_config", [
    "SPLIT_LEGACY_CONFIG",
    "REPAIR_CLASH_CONFIG_PATH",
  ]),
  // -- globals an OS or third-party API imposes -----------------------------
  {
    path: "backend/tauri/src/utils/dock.rs",
    name: "MARK",
    category: "external",
    reason: "AppKit main-thread marker, bound to the thread by the OS API",
  },
  {
    path: "backend/tauri/src/shutdown_hook.rs",
    name: "SHUTDOWN_HOOK_INSTANCE",
    category: "external",
    reason:
      "set once by setup_shutdown_hook, read by a Win32 window procedure, which has no closure state",
  },
  {
    path: "backend/tauri/src/shutdown_hook.rs",
    name: "SHUTDOWN_STATE",
    category: "external",
    reason:
      "read by the Win32 window procedure; written by set_ready_for_shutdown from the exit boundary in utils::exit",
  },
  {
    path: "backend/tauri/src/utils/main_thread.rs",
    name: "MAIN_THREAD_ID",
    category: "external",
    reason:
      "Windows main thread id, written once by a CRT initializer before main and only read after",
  },
  {
    path: "backend/tauri/src/utils/main_thread.rs",
    name: "RECORD_MAIN_THREAD_ID",
    category: "external",
    reason:
      "CRT initializer the linker section points at, which has no closure state",
  },
  {
    path: "backend/tauri/src/main.rs",
    name: "ALLOC",
    category: "external",
    reason:
      "the global allocator Rust requires to be a static; compiled only with the dhat-heap profiling feature",
  },
  // -- test-only ------------------------------------------------------------
  {
    path: "backend/tauri/src/core/migration/runner.rs",
    name: "TEST_VERSION",
    category: "test",
    reason: "constant version in a test fixture",
  },
];

export function staticAllowlistKey(relPath: string, name: string): string {
  return `${relPath}::${name}`;
}

export function isStaticAllowlisted(relPath: string, name: string): boolean {
  return STATIC_ALLOWLIST.some((entry) =>
    entry.path === relPath && entry.name === name
  );
}

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
  /** Statics no allowlist entry covers; the gate wants none. */
  mutableStatics: MetricBucket;
  /** Report-only: which allowlist entries the scan found. */
  allowlistedStatics: MetricBucket;
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
    | "static_allowlist"
    | "snapshot";
  message: string;
};

export type GateResult = {
  ok: boolean;
  issues: GateIssue[];
};

export function rel(filePath: string, root: string): string {
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
