import assert from "node:assert/strict";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { pathExists } from "./path-exists.ts";

test("pathExists reports existing and missing paths", async () => {
  assert.deepStrictEqual(await pathExists(fileURLToPath(import.meta.url)), true);
  assert.deepStrictEqual(await pathExists("/nonexistent/nyanpasu-scripts"), false);
});
