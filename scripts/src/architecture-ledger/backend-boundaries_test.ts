import { assertEquals } from "jsr:@std/assert";
import { dependencyViolations } from "./backend-boundaries.ts";

Deno.test("transitive GUI and host dependencies invalidate neutral packages", () => {
  assertEquals(
    dependencyViolations(
      "nyanpasu-config",
      "nyanpasu-config v0.1.0\nnyanpasu-egui v0.1.0\neframe v0.36.1 (*)\n",
    ),
    [
      "nyanpasu-config must not depend on nyanpasu-egui",
      "nyanpasu-config must not depend on eframe",
    ],
  );
  assertEquals(
    dependencyViolations("nyanpasu-platform", "tauri-specta v2.0.0\n"),
    ["nyanpasu-platform must not depend on tauri-specta"],
  );
  assertEquals(
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

Deno.test("core consumes ports while platform implements them", () => {
  const graph = "nyanpasu-platform v0.1.0\nnyanpasu-core v0.1.0\n";
  assertEquals(dependencyViolations("nyanpasu-platform", graph), []);
  assertEquals(dependencyViolations("nyanpasu-core", graph), [
    "nyanpasu-core must not depend on nyanpasu-platform",
  ]);
});

Deno.test("domain, actor, script and storage dependencies remain available", () => {
  assertEquals(
    dependencyViolations(
      "nyanpasu-platform",
      "nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\nractor v0.16\nboa_engine v0.22\nmlua v0.12\ntokio v1.0\n",
    ),
    [],
  );
});
