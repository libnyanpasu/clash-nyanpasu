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
    dependencyViolations("nyanpasu-core", "tauri-specta v2.0.0\n"),
    ["nyanpasu-core must not depend on tauri-specta"],
  );
});

Deno.test("core consumes config without a reverse domain dependency", () => {
  const graph = "nyanpasu-core v0.1.0\nnyanpasu-config v0.1.0\n";
  assertEquals(dependencyViolations("nyanpasu-core", graph), []);
  assertEquals(dependencyViolations("nyanpasu-config", graph), [
    "nyanpasu-config must not depend on nyanpasu-core",
  ]);
});

Deno.test("domain, actor, script and storage dependencies remain available", () => {
  assertEquals(
    dependencyViolations(
      "nyanpasu-core",
      "nyanpasu-config v0.1.0\nractor v0.16\nboa_engine v0.22\nmlua v0.12\ntokio v1.0\n",
    ),
    [],
  );
});
