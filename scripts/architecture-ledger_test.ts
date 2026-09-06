/**
 * Fixture-only tests for the S10 architecture ledger gate.
 * Does not scan the live repository; inputs are in-memory or temporary fixtures.
 */
import {
  assert,
  assertEquals,
  assertFalse,
  assertThrows,
} from "jsr:@std/assert@1";
import {
  cfgInnerEnablesTest,
  collectRustFiles,
  createBuckets,
  evaluateGate,
  isBridgePath,
  isDedicatedTestPath,
  isLikelyFnDefinition,
  isTestCfgAttr,
  matchRealDirDenylist,
  parseStableSnapshot,
  reportToStableSnapshot,
  scanFile,
  type StableSnapshot,
  testLineMask,
  toStableSnapshot,
} from "./architecture-ledger.ts";

function emptyStable(overrides: Partial<StableSnapshot> = {}): StableSnapshot {
  const baseMetrics: StableSnapshot["metrics"] = {
    config_calls: { total: 0, byKey: {} },
    service_globals: { total: 0, byKey: {} },
    migration_markers: { total: 0, byKey: {} },
    legacy_dto_refs: { total: 0, byKey: {} },
    test_real_dirs: { total: 0, byKey: {} },
  };
  return {
    roots: overrides.roots ?? ["backend"],
    bridgeFiles: overrides.bridgeFiles ?? [],
    metrics: {
      ...baseMetrics,
      ...(overrides.metrics ?? {}),
    },
  };
}

Deno.test("evaluateGate: equal pass", () => {
  const snap = emptyStable({
    bridgeFiles: ["backend/tauri/src/bridge/mod.rs"],
    metrics: {
      config_calls: { total: 2, byKey: { "Config::verge()": 2 } },
      service_globals: { total: 1, byKey: { "CoreManager::global()": 1 } },
      migration_markers: { total: 1, byKey: { "TODO(actor-migration)": 1 } },
      legacy_dto_refs: { total: 1, byKey: { IVerge: 1 } },
      test_real_dirs: { total: 0, byKey: {} },
    },
  });
  const result = evaluateGate(structuredClone(snap), structuredClone(snap));
  assert(result.ok);
  assertEquals(result.issues.length, 0);
});

Deno.test("evaluateGate: metric drift fail", () => {
  const expected = emptyStable({
    metrics: {
      config_calls: { total: 2, byKey: { "Config::verge()": 2 } },
      service_globals: { total: 0, byKey: {} },
      migration_markers: { total: 0, byKey: {} },
      legacy_dto_refs: { total: 0, byKey: {} },
      test_real_dirs: { total: 0, byKey: {} },
    },
  });
  const current = emptyStable({
    metrics: {
      config_calls: {
        total: 3,
        byKey: { "Config::verge()": 2, "Config::clash()": 1 },
      },
      service_globals: { total: 0, byKey: {} },
      migration_markers: { total: 0, byKey: {} },
      legacy_dto_refs: { total: 0, byKey: {} },
      test_real_dirs: { total: 0, byKey: {} },
    },
  });
  const result = evaluateGate(current, expected);
  assertFalse(result.ok);
  assert(
    result.issues.some((i) =>
      i.kind === "metric_total" && i.message.includes("config_calls.total")
    ),
  );
  assert(
    result.issues.some((i) =>
      i.kind === "metric_key" && i.message.includes("Config::clash()")
    ),
  );
});

Deno.test("evaluateGate: bridge drift fail", () => {
  const expected = emptyStable({
    bridgeFiles: [
      "backend/tauri/src/bridge/mod.rs",
      "backend/tauri/src/client/core_bridge.rs",
    ],
  });
  const current = emptyStable({
    bridgeFiles: [
      "backend/tauri/src/bridge/mod.rs",
      "backend/tauri/src/enhance/artifact_bridge.rs",
    ],
  });
  const result = evaluateGate(current, expected);
  assertFalse(result.ok);
  const bridgeIssues = result.issues.filter((i) => i.kind === "bridge_files");
  assert(bridgeIssues.length >= 2);
  assert(
    bridgeIssues.some((i) => i.message.includes("core_bridge.rs")),
  );
  assert(
    bridgeIssues.some((i) => i.message.includes("artifact_bridge.rs")),
  );
});

Deno.test("evaluateGate: hard denylist fail even if snapshot matches non-zero", () => {
  const bad = emptyStable({
    metrics: {
      config_calls: { total: 0, byKey: {} },
      service_globals: { total: 0, byKey: {} },
      migration_markers: { total: 0, byKey: {} },
      legacy_dto_refs: { total: 0, byKey: {} },
      test_real_dirs: {
        total: 1,
        byKey: { "dirs::app_home_dir()": 1 },
      },
    },
  });
  // Snapshot also records residual 1 — still hard-fail.
  const result = evaluateGate(structuredClone(bad), structuredClone(bad));
  assertFalse(result.ok);
  assert(
    result.issues.some((i) =>
      i.kind === "hard_denylist" && i.message.includes("test_real_dirs.total")
    ),
  );
});

Deno.test("evaluateGate: roots drift fail", () => {
  const expected = emptyStable({ roots: ["backend"] });
  const current = emptyStable({ roots: ["backend", "frontend"] });
  const result = evaluateGate(current, expected);
  assertFalse(result.ok);
  assert(result.issues.some((i) => i.kind === "roots"));
});

Deno.test("parseStableSnapshot: missing / invalid fail", () => {
  assertThrows(
    () => parseStableSnapshot(null),
    Error,
    "snapshot must be a JSON object",
  );
  assertThrows(
    () => parseStableSnapshot({ roots: "backend" }),
    Error,
    "snapshot.roots",
  );
  assertThrows(
    () =>
      parseStableSnapshot({
        roots: ["backend"],
        bridgeFiles: [],
        metrics: { config_calls: { total: "x", byKey: {} } },
      }),
    Error,
    "total",
  );
  assertThrows(
    () =>
      parseStableSnapshot({
        roots: ["backend"],
        bridgeFiles: "nope",
        metrics: {},
      }),
    Error,
    "bridgeFiles",
  );
});

Deno.test("parseStableSnapshot: strips report-only fields and accepts stable contract", () => {
  const parsed = parseStableSnapshot({
    generatedAt: "2026-01-01T00:00:00.000Z",
    mode: "report",
    scannedFiles: 999,
    notes: ["ignored"],
    roots: ["backend"],
    bridgeFiles: ["b.rs", "a.rs"],
    metrics: {
      config_calls: {
        total: 1,
        byKey: { "Config::verge()": 1 },
        samples: [{ file: "x.rs", line: 1, text: "ignored" }],
      },
    },
  });
  assertEquals(parsed.roots, ["backend"]);
  assertEquals(parsed.bridgeFiles, ["a.rs", "b.rs"]);
  assertEquals(parsed.metrics.config_calls.total, 1);
  assertEquals(parsed.metrics.config_calls.byKey, { "Config::verge()": 1 });
  assertFalse("samples" in parsed.metrics.config_calls);
  assertFalse("generatedAt" in parsed);
});

Deno.test("toStableSnapshot / reportToStableSnapshot: exclude volatile fields", () => {
  const stable = toStableSnapshot(
    ["backend"],
    {
      config_calls: {
        total: 1,
        byKey: { "Config::verge()": 1 },
        samples: [{ file: "x.rs", line: 1, text: "Config::verge()" }],
      },
    },
    ["z.rs", "a.rs"],
  );
  assertEquals(stable.bridgeFiles, ["a.rs", "z.rs"]);
  assertEquals(
    Object.keys(stable.metrics.config_calls).sort(),
    ["byKey", "total"],
  );

  const fromReport = reportToStableSnapshot({
    generatedAt: "t",
    mode: "gate",
    roots: ["backend"],
    scannedFiles: 1,
    metrics: {
      config_calls: {
        total: 0,
        byKey: {},
        samples: [],
      },
    },
    bridgeFiles: [],
    notes: ["n"],
  });
  assertEquals(fromReport.roots, ["backend"]);
  assertEquals(fromReport.bridgeFiles, []);
});

Deno.test("classification: bridge path boundaries", () => {
  assert(isBridgePath("backend/tauri/src/bridge/mod.rs"));
  assert(isBridgePath("backend/tauri/src/client/core_bridge.rs"));
  assert(isBridgePath("backend/tauri/src/enhance/artifact_bridge.rs"));
  assertFalse(isBridgePath("backend/tauri/src/client/mod.rs"));
  assertFalse(isBridgePath("backend/tauri/src/core/clash/core.rs"));
});

Deno.test("classification: dedicated test path boundaries", () => {
  assert(isDedicatedTestPath("backend/tauri/tests/foo.rs"));
  assert(isDedicatedTestPath("backend/tauri/src/foo_test.rs"));
  assert(isDedicatedTestPath("backend/tauri/src/test.rs"));
  assert(isDedicatedTestPath("backend/tauri/src/utils/tests.rs"));
  assertFalse(isDedicatedTestPath("backend/tauri/src/client/mod.rs"));
  assertFalse(isDedicatedTestPath("backend/tauri/src/testing_helpers.rs"));
  assertFalse(isDedicatedTestPath("backend/tauri/src/contest.rs"));
});

Deno.test("classification: isTestCfgAttr all/any/not(test)", () => {
  assert(isTestCfgAttr("#[cfg(test)]"));
  assert(isTestCfgAttr("#[cfg(all(test, unix))]"));
  assert(isTestCfgAttr("#[cfg(any(windows, test))]"));
  assert(isTestCfgAttr('#[cfg(all(feature = "x", test))]'));
  assertFalse(isTestCfgAttr("#[cfg(not(test))]"));
  assertFalse(isTestCfgAttr("#[cfg(all(not(test), unix))]"));
  assertFalse(isTestCfgAttr('#[cfg(feature = "test")]'));
  assertFalse(isTestCfgAttr("#[test]"));
  assert(cfgInnerEnablesTest("test"));
  assert(cfgInnerEnablesTest("all(test, unix)"));
  assertFalse(cfgInnerEnablesTest("not(test)"));
});

Deno.test("classification: testLineMask scopes cfg(test) without whole-file bleed", () => {
  const source = [
    "use crate::utils::dirs;",
    "#[cfg(test)]",
    "pub use super::helpers;",
    "fn production() {",
    "    let _ = dirs::app_home_dir();",
    "}",
    "#[cfg(test)]",
    "mod tests {",
    "    #[test]",
    "    fn t() {",
    "        let _ = crate::utils::dirs::profiles_path();",
    "    }",
    "}",
  ];
  const mask = testLineMask(source, false);
  // production line with app_home_dir is NOT test
  assertFalse(mask[4]);
  // inside mod tests is test
  assert(mask[10]);
  // cfg(test) pub use item only — not remainder
  assert(mask[1]);
  assert(mask[2]);
  assertFalse(mask[3]);
});

Deno.test("classification: testLineMask honors cfg(all/any test) and ignores not(test)", () => {
  const source = [
    '#[cfg(all(test, feature = "x"))]',
    "mod all_tests {",
    "    fn a() { let _ = app_home_dir(); }",
    "}",
    "#[cfg(any(test, windows))]",
    "fn any_item() {",
    "    let _ = cache_dir();",
    "}",
    "#[cfg(not(test))]",
    "fn prod_only() {",
    "    let _ = app_home_dir();",
    "}",
  ];
  const mask = testLineMask(source, false);
  assert(mask[0]);
  assert(mask[2]);
  assert(mask[4]);
  assert(mask[6]);
  assertFalse(mask[8]);
  assertFalse(mask[10]);
});

Deno.test("matchRealDirDenylist: bare import, qualified, method exclude, fn def exclude", () => {
  const bare = matchRealDirDenylist("    let p = app_home_dir();");
  assertEquals(bare.length, 1);
  assert(bare[0].key.includes("app_home_dir"));

  const qualified = matchRealDirDenylist(
    "let p = crate::utils::dirs::profiles_path();",
  );
  assertEquals(qualified.length, 1);

  const dirs = matchRealDirDenylist("let p = dirs::cache_dir();");
  assertEquals(dirs.length, 1);

  const tray = matchRealDirDenylist('let p = tray_icons_path("sys");');
  assertEquals(tray.length, 1);

  const method = matchRealDirDenylist(
    "let p = paths.app_home_dir(); let q = resolver.cache_dir();",
  );
  assertEquals(method.length, 0);

  const defLine = "pub fn app_home_dir() -> PathBuf {";
  assertEquals(matchRealDirDenylist(defLine).length, 0);
  assert(isLikelyFnDefinition(defLine, defLine.indexOf("app_home_dir")));

  const asyncDef = "pub async fn cache_dir() -> Result<PathBuf> {";
  assertEquals(matchRealDirDenylist(asyncDef).length, 0);
});

Deno.test("scanFile: counts residuals and ignores PathResolver method names", () => {
  const buckets = createBuckets();
  const source = `
use crate::config::Config;
// TODO(actor-migration): temporary bridge.
fn prod(paths: &PathResolver) {
    let _ = Config::verge();
    let _ = CoreManager::global();
    let _ = IVerge::default();
    // injected resolver — must NOT count as denylist
    let _ = paths.profiles_path();
    let _ = paths.app_home_dir();
}
#[cfg(test)]
mod tests {
    #[test]
    fn bad() {
        let _ = crate::utils::dirs::app_home_dir();
        let _ = runtime_config_path();
    }
    #[test]
    fn ok_resolver() {
        let paths = PathResolver::temp();
        let _ = paths.profiles_path();
    }
}
`;
  scanFile("backend/tauri/src/example.rs", source, buckets);

  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::verge()"), 1);
  assertEquals(buckets.serviceGlobals.total, 1);
  assertEquals(buckets.serviceGlobals.byKey.get("CoreManager::global()"), 1);
  assertEquals(buckets.migrationMarkers.total, 1);
  assertEquals(buckets.legacyDtos.total, 1);
  assertEquals(buckets.legacyDtos.byKey.get("IVerge"), 1);

  // denylist: qualified + bare free helpers inside test mask; methods excluded
  assertEquals(buckets.testRealDirs.total, 2);
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) =>
      k.includes("app_home_dir")
    ),
  );
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) =>
      k.includes("runtime_config_path")
    ),
  );
});

Deno.test("scanFile: bare imported denylist calls in test regions", () => {
  const buckets = createBuckets();
  const source = `
use crate::utils::dirs::app_home_dir;
use crate::utils::dirs::cache_dir;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() {
        let _ = app_home_dir();
        let _ = cache_dir();
        let _ = tray_icons_path("normal");
        let _ = paths.app_home_dir();
    }
}
`;
  scanFile("backend/tauri/src/bare.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 3);
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) => k === "app_home_dir()"),
  );
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) => k === "cache_dir()"),
  );
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) =>
      k.startsWith("tray_icons_path(")
    ),
  );
});

Deno.test("scanFile: production real-dir helpers are not denylist hits", () => {
  const buckets = createBuckets();
  const source = `
fn boot() {
    let _ = crate::utils::dirs::app_home_dir();
    let _ = dirs::profiles_path();
    let _ = runtime_config_path();
    let _ = app_home_dir();
    let _ = cache_dir();
}
`;
  scanFile("backend/tauri/src/boot.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 0);
});

Deno.test("scanFile: dedicated test file marks entire file for denylist", () => {
  const buckets = createBuckets();
  const source = `
fn helper() {
    let _ = dirs::app_data_dir();
}
`;
  scanFile("backend/tauri/tests/isolation.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 1);
});

Deno.test("scanFile: sibling tests.rs is a dedicated test path", () => {
  const buckets = createBuckets();
  const source = `
fn helper() {
    let _ = app_home_dir();
    let _ = paths.cache_dir();
}
`;
  scanFile("backend/tauri/src/utils/tests.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 1);
  assertEquals(buckets.testRealDirs.byKey.get("app_home_dir()"), 1);
});

Deno.test("scanFile: block comments do not count code metrics; // TODOs still count", () => {
  const buckets = createBuckets();
  const source = `
/* Config::verge() CoreManager::global() IVerge */
// TODO(actor-migration): still counted
fn x() {}
`;
  scanFile("backend/tauri/src/c.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 0);
  assertEquals(buckets.serviceGlobals.total, 0);
  assertEquals(buckets.legacyDtos.total, 0);
  assertEquals(buckets.migrationMarkers.total, 1);
});

Deno.test("scanFile: a doc comment's literal /core/* does not open a block comment", () => {
  const buckets = createBuckets();
  const source = `
/// see /core/* here
fn x() {
    let _ = Config::verge();
}
`;
  scanFile("backend/tauri/src/d.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::verge()"), 1);
});

Deno.test("scanFile: // inside a string literal does not strip the rest of the line", () => {
  const buckets = createBuckets();
  const source = `
fn x() {
    let url = "http://example.com";
    let _ = Config::verge();
}
`;
  scanFile("backend/tauri/src/e.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::verge()"), 1);
});

Deno.test("scanFile: a real multi-line block comment still hides code between /* and */", () => {
  const buckets = createBuckets();
  const source = `
/* start
   Config::verge() should not count
*/
fn x() {
    let _ = Config::clash();
}
`;
  scanFile("backend/tauri/src/f.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::clash()"), 1);
});

Deno.test("scanFile: even backslash run before a closing quote does not misparse a trailing comment", () => {
  const buckets = createBuckets();
  // The string contains two escaped backslashes (an even run of 4 `\`), so
  // the closing `"` is real and `/* Config::verge() */` is a real comment.
  const source = String.raw`let s = "\\\\"; /* Config::verge() */
`;
  scanFile("backend/tauri/src/escape.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 0);
});

Deno.test("scanFile: a lone lifetime tick does not corrupt string/comment state for the rest of the line", () => {
  const buckets = createBuckets();
  const source = `let x: &'static str = "s"; /* Config::verge() */\n`;
  scanFile("backend/tauri/src/lifetime.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 0);
});

Deno.test("scanFile: comment-like text inside a raw string is not treated as a real comment", () => {
  const buckets = createBuckets();
  const source =
    `let s = r#"contains "// looks like a comment" text"#; let _ = Config::verge();\n`;
  scanFile("backend/tauri/src/rawstr.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::verge()"), 1);
});

Deno.test("scanFile: nested block comments only close at the matching outer */", () => {
  const buckets = createBuckets();
  const source = `
/* outer /* inner */ still outer Config::verge() */
Config::clash();
`;
  scanFile("backend/tauri/src/nested.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::clash()"), 1);
  assertEquals(buckets.configCalls.byKey.get("Config::verge()"), undefined);
});

Deno.test("scanFile: fn definitions of denylist helpers are not hits", () => {
  const buckets = createBuckets();
  const source = `
pub fn app_home_dir() -> PathBuf { PathBuf::new() }
pub async fn cache_dir() -> Result<PathBuf> { todo!() }
fn tray_icons_path(mode: &str) -> PathBuf { PathBuf::from(mode) }
`;
  scanFile("backend/tauri/src/utils/tests.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 0);
});

// Reviewer finding (Major): a lifetime apostrophe on a `#[cfg(test)]` item's
// signature line must not make `stripLineComment`'s old quote tracking treat
// a trailing `// }` as unstripped code — that fake `}` closed the item early
// and dropped `app_home_dir()` out of the test mask, undercounting the hard
// denylist. Sharing the literal-aware lexer with brace/item scanning fixes
// this; the file path is deliberately not a dedicated test path so the bug
// can't be masked by whole-file test detection.
Deno.test("scanFile: lifetime in a cfg(test) fn signature does not truncate the item mask at a fake `// }`", () => {
  const buckets = createBuckets();
  const source = `
#[cfg(test)]
fn isolated<'a>() { // }
    let _ = app_home_dir();
}
`;
  scanFile("backend/tauri/src/lifetime_mask.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 1);
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) =>
      k.includes("app_home_dir")
    ),
  );
});

// Reviewer finding (Minor): raw C strings (`cr"..."`, `cr#"..."#`) were not
// recognized as raw-string openers, so the opening `"` was treated as a
// normal string. An odd backslash before the closing `"#` then left the
// (fake) normal string unterminated for the rest of the line, swallowing the
// real `Config::global()` call that follows.
Deno.test('scanFile: cr#"..."# raw C string opener does not swallow real code after it', () => {
  const buckets = createBuckets();
  const source = String.raw`let value = cr#"\"#; Config::global();
`;
  scanFile("backend/tauri/src/raw_c_string.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 1);
  assertEquals(buckets.configCalls.byKey.get("Config::global()"), 1);
  assertEquals(buckets.serviceGlobals.total, 1);
  assertEquals(buckets.serviceGlobals.byKey.get("Config::global()"), 1);
});

// Reviewer finding (Minor): string/raw-string/char-literal contents were
// copied verbatim into `code` before the metric regexes ran, so metric-like
// text inside a literal produced a false positive.
Deno.test("scanFile: metric-like text inside a raw string literal is not counted", () => {
  const buckets = createBuckets();
  const source = `let example = r#"Config::global()"#;\n`;
  scanFile("backend/tauri/src/literal_text.rs", source, buckets);
  assertEquals(buckets.configCalls.total, 0);
  assertEquals(buckets.serviceGlobals.total, 0);
});

// Bonus correctness win from sharing the lexer: a `}` inside a string
// literal must not feed brace/item scanning, or it closes a cfg(test) item
// early and drops the real code after it out of the test mask — the same
// undercount shape as the Major finding, triggered by a brace instead of a
// lifetime.
Deno.test("scanFile: an unmatched brace inside a string literal does not end cfg(test) item scanning early", () => {
  const buckets = createBuckets();
  const source = `
#[cfg(test)]
fn braces_in_string() {
    let _ = "unexpected } inside string";
    let _ = app_home_dir();
}
`;
  scanFile("backend/tauri/src/brace_in_string.rs", source, buckets);
  assertEquals(buckets.testRealDirs.total, 1);
  assert(
    [...buckets.testRealDirs.byKey.keys()].some((k) =>
      k.includes("app_home_dir")
    ),
  );
});

Deno.test("collectRustFiles: a tmp ancestor does not exclude the worktree", async () => {
  const fixture = await Deno.makeTempDir();
  const repo = `${fixture}/tmp/worktree`;
  try {
    for (const dir of ["src", "tmp", "target", "nyanpasu-runtime"]) {
      await Deno.mkdir(`${repo}/backend/${dir}`, { recursive: true });
      await Deno.writeTextFile(
        `${repo}/backend/${dir}/sample.rs`,
        "fn sample() {}",
      );
    }
    const files = await collectRustFiles(["backend"], repo);
    assertEquals(files.length, 1);
    assertEquals(
      await Deno.realPath(files[0]),
      await Deno.realPath(`${repo}/backend/src/sample.rs`),
    );
  } finally {
    await Deno.remove(fixture, { recursive: true });
  }
});
