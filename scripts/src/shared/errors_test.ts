import { assertEquals } from "jsr:@std/assert@1";
import { readFile } from "node:fs/promises";
import { isNotFound } from "./errors.ts";

Deno.test("isNotFound recognizes a missing path only", async () => {
  const missing = await readFile("/nonexistent/nyanpasu-scripts").catch((e) =>
    e
  );
  assertEquals(isNotFound(missing), true);
  assertEquals(isNotFound(new Error("other")), false);
  assertEquals(isNotFound("ENOENT"), false);
});
