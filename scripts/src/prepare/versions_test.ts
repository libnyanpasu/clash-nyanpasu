import assert from "node:assert/strict";
import { test } from "node:test";
import {
  extractVersionFromAssetList,
  isValidVersion,
  normalizeVersion,
} from "./versions.ts";

test("normalizes and validates versions", () => {
  assert.deepStrictEqual(normalizeVersion(), undefined);
  assert.deepStrictEqual(normalizeVersion("Not Found"), undefined);
  assert.deepStrictEqual(normalizeVersion("v1.2.3"), "v1.2.3");
  assert.deepStrictEqual(isValidVersion("v1.2.3-rc.1+build"), true);
  assert.deepStrictEqual(isValidVersion("bad/version"), false);
});

test("extracts a version matching an asset template", () => {
  assert.deepStrictEqual(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-v1.19.0-alpha.2.gz other asset",
      "mihomo-linux-amd64-{}.gz",
    ),
    "v1.19.0-alpha.2",
  );
  assert.deepStrictEqual(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-latest.gz",
      "no-placeholder",
    ),
    undefined,
  );
  assert.deepStrictEqual(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-bad/version.gz",
      "mihomo-linux-amd64-{}.gz",
    ),
    undefined,
  );
});
