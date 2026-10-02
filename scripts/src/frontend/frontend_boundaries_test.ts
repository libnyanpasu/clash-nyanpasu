import { describe, it } from "jsr:@std/testing/bdd";
import { assert, assertEquals, assertStringIncludes } from "jsr:@std/assert";
import { checkFrontendBoundaries, type FrontendPackage } from "./policy.ts";

const root = "/repo";

function sharedPackage(
  name: string,
  options: Partial<FrontendPackage> = {},
): FrontendPackage {
  const segment = name.slice("@nyanpasu/".length);
  return {
    name,
    path: `frontend/${segment}`,
    role: "shared",
    private: true,
    exports: { ".": "./src/index.ts" },
    dependencies: {},
    sources: [],
    ...options,
  };
}

describe("frontend package boundaries", () => {
  it("accepts source exports and a declared acyclic workspace dependency", () => {
    const inventory = [
      sharedPackage("@nyanpasu/constants"),
      sharedPackage("@nyanpasu/hooks", {
        dependencies: { "@nyanpasu/constants": "workspace:*" },
        sources: [{
          path: "frontend/hooks/src/index.ts",
          imports: ["@nyanpasu/constants"],
        }],
      }),
      {
        ...sharedPackage("@nyanpasu/nyanpasu"),
        role: "app" as const,
      },
    ];

    assertEquals(checkFrontendBoundaries(inventory, root), []);
  });

  it("reports a dependency cycle as one concrete path", () => {
    const inventory = [
      sharedPackage("@nyanpasu/hooks", {
        dependencies: { "@nyanpasu/theme": "workspace:^" },
      }),
      sharedPackage("@nyanpasu/theme", {
        dependencies: { "@nyanpasu/hooks": "workspace:^" },
      }),
    ];

    const issues = checkFrontendBoundaries(inventory, root);
    assertEquals(issues.length, 1);
    assertStringIncludes(
      issues[0],
      "workspace dependency cycle: @nyanpasu/hooks -> @nyanpasu/theme -> @nyanpasu/hooks",
    );
  });

  it("rejects an imported workspace package with no direct workspace declaration", () => {
    const inventory = [
      sharedPackage("@nyanpasu/hooks", {
        sources: [{
          path: "frontend/hooks/src/index.ts",
          imports: ["@nyanpasu/platform/window"],
        }],
      }),
      sharedPackage("@nyanpasu/platform"),
    ];

    const issues = checkFrontendBoundaries(inventory, root);
    assert(
      issues.some((issue) =>
        issue.includes(
          "import @nyanpasu/platform without a workspace dependency",
        )
      ),
    );
  });

  it("catches app aliases and relative imports back into the app", () => {
    const issues = checkFrontendBoundaries([
      sharedPackage("@nyanpasu/utils", {
        sources: [{
          path: "frontend/utils/src/index.ts",
          imports: [
            "@/utils/notification",
            "../../nyanpasu/src/components/app",
          ],
        }],
      }),
    ], root);

    assert(
      issues.some((issue) =>
        issue.includes("app-only import @/utils/notification")
      ),
    );
    assert(
      issues.some((issue) => issue.includes("import into frontend/nyanpasu")),
    );
  });

  it("rejects shared packages whose export and dependency boundaries are wrong", () => {
    const issues = checkFrontendBoundaries([
      sharedPackage("@nyanpasu/hooks", {
        private: false,
        exports: { ".": "./dist/index.js" },
        dependencies: { "@nyanpasu/query": "workspace:^" },
      }),
      sharedPackage("@nyanpasu/query"),
    ], root);

    assert(issues.some((issue) => issue.includes("must be private")));
    assert(
      issues.some((issue) =>
        issue.includes("exports must point to package source")
      ),
    );
    assert(
      issues.some((issue) => issue.includes("dependency on @nyanpasu/query")),
    );
  });

  it("allows generated schema type imports from local source modules", () => {
    const issues = checkFrontendBoundaries([
      sharedPackage("@nyanpasu/constants", {
        sources: [{
          path: "frontend/constants/src/index.ts",
          imports: ["./types"],
          source: "export type { GeneratedSchema } from './types'",
        }],
      }),
    ], root);

    assertEquals(issues, []);
  });

  it("rejects DOM access in shared utilities", () => {
    const issues = checkFrontendBoundaries([
      sharedPackage("@nyanpasu/constants", {
        sources: [{
          path: "frontend/constants/src/index.ts",
          imports: [],
          source: "window.location.href",
        }],
      }),
    ], root);

    assertEquals(issues, [
      "@nyanpasu/constants: DOM access in frontend/constants/src/index.ts",
    ]);
  });
  it("rejects legacy packages and undeclared imports of unknown packages", () => {
    const issues = checkFrontendBoundaries([
      sharedPackage("@nyanpasu/interface"),
      sharedPackage("@nyanpasu/hooks", {
        sources: [{
          path: "frontend/hooks/src/index.ts",
          imports: ["@nyanpasu/missing"],
        }],
      }),
    ], root);
    assert(
      issues.some((issue) => issue.includes("legacy package must be removed")),
    );
    assert(
      issues.some((issue) =>
        issue.includes(
          "import @nyanpasu/missing without a workspace dependency",
        )
      ),
    );
  });
});
