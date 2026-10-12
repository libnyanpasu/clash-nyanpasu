import { assertEquals } from "jsr:@std/assert@1";
import { changedParts, type Part, partsOf } from "./changed-parts.ts";

const sorted = (parts: Iterable<Part>) => [...parts].sort();

Deno.test("files map to the parts that check them", () => {
  const cases: [string, Part[]][] = [
    ["README.md", []],
    ["docs/development/workflow.md", []],
    ["renovate.json", []],
    ["frontend/nyanpasu/src/main.tsx", ["frontend"]],
    ["frontend/README.md", ["frontend"]],
    ["pnpm-lock.yaml", ["frontend"]],
    ["tsconfig.test.json", ["frontend"]],
    ["frontend/rpc/src/rpc-bindings.ts", ["backend", "frontend"]],
    ["frontend/query/src/query-bindings.ts", ["backend", "frontend"]],
    ["backend/tauri/src/main.rs", ["backend"]],
    ["backend/nyanpasu-runtime", ["backend"]],
    ["backend/README.md", ["backend"]],
    ["rust-toolchain.toml", ["backend"]],
    ["scripts/src/ci/changed-parts.ts", ["scripts"]],
    ["deno.jsonc", ["scripts"]],
    ["manifest/version.json", ["scripts"]],
  ];
  for (const [file, parts] of cases) {
    assertEquals(sorted(partsOf(file)), sorted(parts), file);
  }
});

Deno.test("unclassified files check every part", () => {
  for (
    const file of [
      ".github/workflows/ci.yml",
      ".gitattributes",
      "tools/log-index-bench/src/main.rs",
    ]
  ) {
    assertEquals(
      sorted(partsOf(file)),
      ["backend", "frontend", "scripts"],
      file,
    );
  }
});

Deno.test("a change checks the union of its files' parts", () => {
  // A grouped Tauri update touches npm manifests and the Cargo lockfile.
  assertEquals(
    sorted(
      changedParts([
        "package.json",
        "frontend/rpc/package.json",
        "pnpm-lock.yaml",
        "backend/Cargo.lock",
      ]),
    ),
    ["backend", "frontend"],
  );
  assertEquals(sorted(changedParts(["README.md", "docs/a.md"])), []);
  assertEquals(sorted(changedParts([])), []);
});
