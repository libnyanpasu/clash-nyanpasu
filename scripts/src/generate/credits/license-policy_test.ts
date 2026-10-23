import assert from "node:assert/strict";
import { test } from "node:test";
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

test("license choice prefers the alternative ranked first", () => {
  assert.deepStrictEqual(choose("MIT OR Apache-2.0"), ["Apache-2.0"]);
  assert.deepStrictEqual(choose("(MPL-2.0 OR MIT)"), ["MIT"]);
  assert.deepStrictEqual(choose("MIT OR (Apache-2.0 AND MPL-2.0)"), ["MIT"]);
});

test("license choice needs every conjunct and exact exceptions", () => {
  assert.deepStrictEqual(choose("MIT AND Apache-2.0"), ["MIT", "Apache-2.0"]);
  assert.deepStrictEqual(choose("MIT AND MPL-2.0"), null);
  assert.deepStrictEqual(choose("Apache-2.0 WITH LLVM-exception"), [
    "Apache-2.0 WITH LLVM-exception",
  ]);
  assert.deepStrictEqual(choose("MIT WITH LLVM-exception"), null);
});

test("license expressions are parsed leniently but must be SPDX", () => {
  assert.deepStrictEqual(choose("MIT  OR  Apache-2.0"), ["Apache-2.0"]);
  assert.deepStrictEqual(parseLicense("Unknown"), null);
  assert.deepStrictEqual(licenseIds(parseLicense("MIT OR (ISC AND Zlib)")!), [
    "MIT",
    "ISC",
    "Zlib",
  ]);
});

test("standard texts ignore exceptions and custom references", () => {
  assert.deepStrictEqual(
    standardLicenseText("Apache-2.0 WITH LLVM-exception"),
    standardLicenseText("Apache-2.0"),
  );
  assert.deepStrictEqual(standardLicenseText("LicenseRef-Custom"), undefined);
});

test("accepted licenses come from about.toml", () => {
  assert.deepStrictEqual(readAcceptedLicenses('accepted = ["MIT", "ISC"]'), [
    "MIT",
    "ISC",
  ]);
  assert.throws(() => readAcceptedLicenses("accepted = [1]"), Error);
  assert.throws(() => readAcceptedLicenses("targets = []"), Error);
});
