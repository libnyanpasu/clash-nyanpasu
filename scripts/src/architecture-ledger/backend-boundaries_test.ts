import assert from "node:assert/strict";
import { test } from "node:test";
import { dependencyViolations } from "./backend-boundaries.ts";

test("transitive GUI and host dependencies invalidate neutral packages", () => {
  assert.deepStrictEqual(
    dependencyViolations(
      "nyanpasu-config",
      "nyanpasu-config v0.1.0\nnyanpasu-egui v0.1.0\neframe v0.36.1 (*)\n",
    ),
    [
      "nyanpasu-config must not depend on nyanpasu-egui",
      "nyanpasu-config must not depend on eframe",
    ],
  );
  assert.deepStrictEqual(
    dependencyViolations("nyanpasu-platform", "tauri-specta v2.0.0\n"),
    ["nyanpasu-platform must not depend on tauri-specta"],
  );
  assert.deepStrictEqual(
    dependencyViolations(
      "nyanpasu-core",
      "nyanpasu-widget v0.1.0\nslint v1.18.1\ni-slint-core v1.18.1\nwinit v0.30.13\n",
    ),
    [
      "nyanpasu-core must not depend on nyanpasu-widget",
      "nyanpasu-core must not depend on slint",
      "nyanpasu-core must not depend on i-slint-core",
      "nyanpasu-core must not depend on winit",
    ],
  );
});

test("core consumes ports while platform implements them", () => {
  const graph = "nyanpasu-platform v0.1.0\nnyanpasu-core v0.1.0\n";
  assert.deepStrictEqual(dependencyViolations("nyanpasu-platform", graph), []);
  assert.deepStrictEqual(dependencyViolations("nyanpasu-core", graph), [
    "nyanpasu-core must not depend on nyanpasu-platform",
  ]);
});

test("domain, actor, script and storage dependencies remain available", () => {
  assert.deepStrictEqual(
    dependencyViolations(
      "nyanpasu-platform",
      "nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\nractor v0.16\nboa_engine v0.22\nmlua v0.12\ntokio v1.0\n",
    ),
    [],
  );
});
