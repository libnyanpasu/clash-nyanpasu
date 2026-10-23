import { assertEquals } from "jsr:@std/assert@1";
import { fileURLToPath } from "node:url";
import { pathExists } from "./path-exists.ts";

Deno.test("pathExists reports existing and missing paths", async () => {
  assertEquals(await pathExists(fileURLToPath(import.meta.url)), true);
  assertEquals(await pathExists("/nonexistent/nyanpasu-scripts"), false);
});
