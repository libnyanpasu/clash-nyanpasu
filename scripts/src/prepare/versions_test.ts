import { assertEquals } from "jsr:@std/assert@1";
import {
  extractVersionFromAssetList,
  isValidVersion,
  normalizeVersion,
} from "./versions.ts";

Deno.test("normalizes and validates versions", () => {
  assertEquals(normalizeVersion(), undefined);
  assertEquals(normalizeVersion("Not Found"), undefined);
  assertEquals(normalizeVersion("v1.2.3"), "v1.2.3");
  assertEquals(isValidVersion("v1.2.3-rc.1+build"), true);
  assertEquals(isValidVersion("bad/version"), false);
});

Deno.test("extracts a version matching an asset template", () => {
  assertEquals(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-v1.19.0-alpha.2.gz other asset",
      "mihomo-linux-amd64-{}.gz",
    ),
    "v1.19.0-alpha.2",
  );
  assertEquals(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-latest.gz",
      "no-placeholder",
    ),
    undefined,
  );
  assertEquals(
    extractVersionFromAssetList(
      "mihomo-linux-amd64-bad/version.gz",
      "mihomo-linux-amd64-{}.gz",
    ),
    undefined,
  );
});
