import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import {
  chooseLicenses,
  licenseIds,
  parseLicense,
  readAcceptedLicenses,
  standardLicenseText,
} from "./license-policy.ts";

const ACCEPTED = ["Apache-2.0", "MIT", "Apache-2.0 WITH LLVM-exception"];

function choose(expression: string): string[] | null {
  return chooseLicenses(parseLicense(expression)!, ACCEPTED);
}

Deno.test("license choice prefers the alternative ranked first", () => {
  assertEquals(choose("MIT OR Apache-2.0"), ["Apache-2.0"]);
  assertEquals(choose("(MPL-2.0 OR MIT)"), ["MIT"]);
  assertEquals(choose("MIT OR (Apache-2.0 AND MPL-2.0)"), ["MIT"]);
});

Deno.test("license choice needs every conjunct and exact exceptions", () => {
  assertEquals(choose("MIT AND Apache-2.0"), ["MIT", "Apache-2.0"]);
  assertEquals(choose("MIT AND MPL-2.0"), null);
  assertEquals(choose("Apache-2.0 WITH LLVM-exception"), [
    "Apache-2.0 WITH LLVM-exception",
  ]);
  assertEquals(choose("MIT WITH LLVM-exception"), null);
});

Deno.test("license expressions are parsed leniently but must be SPDX", () => {
  assertEquals(choose("MIT  OR  Apache-2.0"), ["Apache-2.0"]);
  assertEquals(parseLicense("Unknown"), null);
  assertEquals(licenseIds(parseLicense("MIT OR (ISC AND Zlib)")!), [
    "MIT",
    "ISC",
    "Zlib",
  ]);
});

Deno.test("standard texts ignore exceptions and custom references", () => {
  assertEquals(
    standardLicenseText("Apache-2.0 WITH LLVM-exception"),
    standardLicenseText("Apache-2.0"),
  );
  assertEquals(standardLicenseText("LicenseRef-Custom"), undefined);
});

Deno.test("accepted licenses come from about.toml", () => {
  assertEquals(readAcceptedLicenses('accepted = ["MIT", "ISC"]'), [
    "MIT",
    "ISC",
  ]);
  assertThrows(() => readAcceptedLicenses("accepted = [1]"), Error);
  assertThrows(() => readAcceptedLicenses("targets = []"), Error);
});
