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
});

Deno.test("application consumes ports while platform implements them", () => {
  const graph = "nyanpasu-platform v0.1.0\nnyanpasu-application v0.1.0\n";
  assertEquals(dependencyViolations("nyanpasu-platform", graph), []);
  assertEquals(dependencyViolations("nyanpasu-application", graph), [
    "nyanpasu-application must not depend on nyanpasu-platform",
  ]);
});

Deno.test("domain, actor, script and storage dependencies remain available", () => {
  assertEquals(
    dependencyViolations(
      "nyanpasu-platform",
      "nyanpasu-config v0.1.0\nractor v0.16\nboa_engine v0.22\nmlua v0.12\ntokio v1.0\n",
    ),
    [],
  );
});
