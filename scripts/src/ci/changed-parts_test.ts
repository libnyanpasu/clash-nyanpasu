import assert from "node:assert/strict";
import { test } from "node:test";
import { changedParts, type Part, partsOf } from "./changed-parts.ts";

const sorted = (parts: Iterable<Part>) => [...parts].sort();

test("files map to the parts that check them", () => {
  const cases: [string, Part[]][] = [
    ["README.md", []],
    ["docs/development/workflow.md", []],
    ["renovate.json", []],
    ["frontend/nyanpasu/src/main.tsx", ["frontend"]],
    ["frontend/README.md", ["frontend"]],
    ["package.json", ["frontend", "scripts"]],
    ["pnpm-lock.yaml", ["frontend", "scripts"]],
    [".oxlintrc.json", ["frontend", "scripts"]],
    ["tsconfig.test.json", ["frontend", "scripts"]],
    ["frontend/rpc/package.json", ["frontend"]],
    ["frontend/rpc/src/rpc-bindings.ts", ["backend", "frontend"]],
    ["frontend/query/src/query-bindings.ts", ["backend", "frontend"]],
    ["backend/tauri/src/main.rs", ["backend"]],
    ["backend/nyanpasu-runtime", ["backend"]],
    ["backend/enable-loopback", ["backend"]],
    ["backend/README.md", ["backend"]],
    ["rust-toolchain.toml", ["backend"]],
    ["scripts/src/ci/changed-parts.ts", ["scripts"]],
    ["scripts/package.json", ["scripts"]],
    ["manifest/version.json", ["scripts"]],
    ["credits/components.toml", ["scripts"]],
  ];
  for (const [file, parts] of cases) {
    assert.deepStrictEqual(sorted(partsOf(file)), sorted(parts), file);
  }
});

test("unclassified files check every part", () => {
  for (
    const file of [
      ".github/workflows/ci.yml",
      ".gitattributes",
      "tools/log-index-bench/src/main.rs",
    ]
  ) {
    assert.deepStrictEqual(
      sorted(partsOf(file)),
      ["backend", "frontend", "scripts"],
      file,
    );
  }
});

test("a change checks the union of its files' parts", () => {
  // A grouped Tauri update touches npm manifests and the Cargo lockfile.
  assert.deepStrictEqual(
    sorted(
      changedParts([
        "package.json",
        "frontend/rpc/package.json",
        "pnpm-lock.yaml",
        "backend/Cargo.lock",
      ]),
    ),
    ["backend", "frontend", "scripts"],
  );
  assert.deepStrictEqual(sorted(changedParts(["README.md", "docs/a.md"])), []);
  assert.deepStrictEqual(sorted(changedParts([])), []);
});
