import assert from "node:assert/strict";
import { test } from "node:test";
import { linuxPackageVersions } from "./prepare-release.ts";

test("stable releases keep one version everywhere", () => {
  assert.deepStrictEqual(linuxPackageVersions("2.0.0"), {
    tauriVersion: "2.0.0",
    rpmRelease: "1",
    debVersion: "2.0.0",
  });
});

test("prereleases sort before the release in rpm and dpkg", () => {
  // Orderings checked with `rpm.vercmp` and `dpkg --compare-versions`:
  // 2.0.0-0.beta.1 < 2.0.0-0.rc.1 < 2.0.0-1 and 2.0.0~beta.1 < 2.0.0~rc.1 < 2.0.0.
  assert.deepStrictEqual(linuxPackageVersions("2.0.0-beta.1"), {
    tauriVersion: "2.0.0",
    rpmRelease: "0.beta.1",
    debVersion: "2.0.0~beta.1",
  });
  assert.deepStrictEqual(linuxPackageVersions("2.1.0-rc.2"), {
    tauriVersion: "2.1.0",
    rpmRelease: "0.rc.2",
    debVersion: "2.1.0~rc.2",
  });
});

test("release versions must be plain semver", () => {
  assert.throws(() => linuxPackageVersions("2.0.0-alpha+abc"));
  assert.throws(() => linuxPackageVersions("v2"));
});
