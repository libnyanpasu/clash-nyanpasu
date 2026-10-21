import { assertEquals } from "jsr:@std/assert@1";
import {
  callersOf,
  type CpuProfile,
  sourceOf,
  summarizeProfile,
} from "./profile-report.ts";

const origin = "http://localhost:63315";
const root = `${origin}/@fs/G:/repo`;

Deno.test("sourceOf names the app, the workspace packages and dependencies", () => {
  assertEquals(
    [
      `${origin}/src/pages/logs/index.tsx?t=1`,
      `${origin}/perf/measure.ts`,
      `${root}/frontend/nyanpasu/src/utils/language.ts`,
      `${root}/frontend/ui/src/button.tsx`,
      `${root}/node_modules/.vite/vitest/abc/deps/motion_react.js?v=1`,
      `${root}/node_modules/.pnpm/@vitest+browser@5/node_modules/@vitest/browser/dist/client.js`,
      "",
    ].map(sourceOf),
    [
      "pages/logs/index.tsx",
      "perf/measure.ts",
      "utils/language.ts",
      "ui/button.tsx",
      "motion_react",
      "@vitest/browser",
      "(native)",
    ],
  );
});

// (root) > render (app) > measure (dependency) > (program) siblings.
const profile: CpuProfile = {
  nodes: [
    {
      id: 1,
      callFrame: { functionName: "(root)", url: "", lineNumber: -1 },
      children: [2, 4],
    },
    {
      id: 2,
      callFrame: {
        functionName: "render",
        url: `${origin}/src/page.tsx`,
        lineNumber: 9,
      },
      children: [3],
    },
    {
      id: 3,
      callFrame: {
        functionName: "measure",
        url: `${root}/node_modules/.vite/vitest/abc/deps/virtual.js`,
        lineNumber: 99,
      },
    },
    {
      id: 4,
      callFrame: { functionName: "(program)", url: "", lineNumber: -1 },
    },
  ],
  samples: [2, 3, 3, 4],
  timeDeltas: [1000, 2000, 2000, 4000],
};

Deno.test("summarizeProfile splits the time per action", () => {
  assertEquals(summarizeProfile(profile, { actions: 2 }), {
    categories: [
      { name: "script", ms: 2.5 },
      { name: "(program)", ms: 2 },
    ],
    sources: [
      { name: "virtual", ms: 2 },
      { name: "page.tsx", ms: 0.5 },
    ],
    functions: [
      { name: "measure virtual:100", ms: 2 },
      { name: "render page.tsx:10", ms: 0.5 },
    ],
  });
});

Deno.test("callersOf lists the stacks under a function's own time", () => {
  assertEquals(callersOf(profile, "measure"), [
    { name: "render page.tsx:10 < (root) (native):0", ms: 4 },
  ]);
});
