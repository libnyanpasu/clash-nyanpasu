import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { resolveNextVersions } from "./publish.ts";

Deno.test("stable bumps keep the existing version scheme", () => {
  assertEquals(resolveNextVersions("2.0.0", "patch", "beta"), {
    version: "2.0.1",
    nightlyVersion: "2.0.2",
  });
  assertEquals(resolveNextVersions("2.0.3", "minor", "beta").version, "2.1.0");
  assertEquals(resolveNextVersions("2.0.3", "major", "beta").version, "3.0.0");
});

Deno.test("prereleases start at 1 and nightly sorts after them", () => {
  assertEquals(resolveNextVersions("2.0.0", "preminor", "beta"), {
    version: "2.1.0-beta.1",
    nightlyVersion: "2.1.1",
  });
  assertEquals(
    resolveNextVersions("2.0.0", "premajor", "rc").version,
    "3.0.0-rc.1",
  );
  assertEquals(
    resolveNextVersions("2.0.0", "prepatch", "beta").version,
    "2.0.1-beta.1",
  );
});

Deno.test("prerelease iterates, switches identifiers, and finalizes", () => {
  assertEquals(
    resolveNextVersions("2.1.0-beta.1", "prerelease", "beta").version,
    "2.1.0-beta.2",
  );
  assertEquals(
    resolveNextVersions("2.1.0-beta.2", "prerelease", "rc").version,
    "2.1.0-rc.1",
  );
  assertEquals(
    resolveNextVersions("2.0.0", "prerelease", "beta").version,
    "2.0.1-beta.1",
  );
  assertEquals(
    resolveNextVersions("2.1.0-rc.1", "minor", "beta").version,
    "2.1.0",
  );
  assertEquals(
    resolveNextVersions("2.0.1-rc.1", "patch", "beta").version,
    "2.0.1",
  );
});

Deno.test("the 2.0.0 manifest publishes 2.0.0-beta.1 first", () => {
  assertEquals(resolveNextVersions("2.0.0-beta.0", "prerelease", "beta"), {
    version: "2.0.0-beta.1",
    nightlyVersion: "2.0.1",
  });
  assertEquals(
    resolveNextVersions("2.0.0-beta.0", "prerelease", "rc").version,
    "2.0.0-rc.1",
  );
  assertEquals(resolveNextVersions("2.0.0-beta.0", "major", "beta"), {
    version: "2.0.0",
    nightlyVersion: "2.0.1",
  });
});

Deno.test("versions never go backwards", () => {
  assertThrows(
    () => resolveNextVersions("2.0.0-rc.1", "prerelease", "beta"),
    Error,
    "does not advance",
  );
});

Deno.test("invalid inputs are rejected", () => {
  // deno-lint-ignore no-explicit-any
  assertThrows(() => resolveNextVersions("2.0.0", "nightly" as any, "beta"));
  // deno-lint-ignore no-explicit-any
  assertThrows(() =>
    resolveNextVersions("2.0.0", "prerelease", "alpha" as any)
  );
  assertThrows(() => resolveNextVersions("not-a-version", "patch", "beta"));
});
