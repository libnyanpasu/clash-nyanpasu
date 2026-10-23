import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveNextVersions } from "./publish.ts";

test("stable bumps keep the existing version scheme", () => {
  assert.deepStrictEqual(resolveNextVersions("2.0.0", "patch", "beta"), {
    version: "2.0.1",
    nightlyVersion: "2.0.2",
  });
  assert.deepStrictEqual(resolveNextVersions("2.0.3", "minor", "beta").version, "2.1.0");
  assert.deepStrictEqual(resolveNextVersions("2.0.3", "major", "beta").version, "3.0.0");
});

test("prereleases start at 1 and nightly sorts after them", () => {
  assert.deepStrictEqual(resolveNextVersions("2.0.0", "preminor", "beta"), {
    version: "2.1.0-beta.1",
    nightlyVersion: "2.1.1",
  });
  assert.deepStrictEqual(
    resolveNextVersions("2.0.0", "premajor", "rc").version,
    "3.0.0-rc.1",
  );
  assert.deepStrictEqual(
    resolveNextVersions("2.0.0", "prepatch", "beta").version,
    "2.0.1-beta.1",
  );
});

test("prerelease iterates, switches identifiers, and finalizes", () => {
  assert.deepStrictEqual(
    resolveNextVersions("2.1.0-beta.1", "prerelease", "beta").version,
    "2.1.0-beta.2",
  );
  assert.deepStrictEqual(
    resolveNextVersions("2.1.0-beta.2", "prerelease", "rc").version,
    "2.1.0-rc.1",
  );
  assert.deepStrictEqual(
    resolveNextVersions("2.0.0", "prerelease", "beta").version,
    "2.0.1-beta.1",
  );
  assert.deepStrictEqual(
    resolveNextVersions("2.1.0-rc.1", "minor", "beta").version,
    "2.1.0",
  );
  assert.deepStrictEqual(
    resolveNextVersions("2.0.1-rc.1", "patch", "beta").version,
    "2.0.1",
  );
});

test("the 2.0.0 manifest publishes 2.0.0-beta.1 first", () => {
  assert.deepStrictEqual(resolveNextVersions("2.0.0-beta.0", "prerelease", "beta"), {
    version: "2.0.0-beta.1",
    nightlyVersion: "2.0.1",
  });
  assert.deepStrictEqual(
    resolveNextVersions("2.0.0-beta.0", "prerelease", "rc").version,
    "2.0.0-rc.1",
  );
  assert.deepStrictEqual(resolveNextVersions("2.0.0-beta.0", "major", "beta"), {
    version: "2.0.0",
    nightlyVersion: "2.0.1",
  });
});

test("versions never go backwards", () => {
  assert.throws(() => resolveNextVersions("2.0.0-rc.1", "prerelease", "beta"), { message: /does not advance/ });
});

test("invalid inputs are rejected", () => {
  assert.throws(() => resolveNextVersions("2.0.0", "nightly" as any, "beta"));
  assert.throws(() =>
    resolveNextVersions("2.0.0", "prerelease", "alpha" as any));
  assert.throws(() => resolveNextVersions("not-a-version", "patch", "beta"));
});
