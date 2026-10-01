import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { linuxPackageVersions } from "./prepare-release.ts";

Deno.test("stable releases keep one version everywhere", () => {
  assertEquals(linuxPackageVersions("2.0.0"), {
    tauriVersion: "2.0.0",
    rpmRelease: "1",
    debVersion: "2.0.0",
  });
});

Deno.test("prereleases sort before the release in rpm and dpkg", () => {
  // Orderings checked with `rpm.vercmp` and `dpkg --compare-versions`:
  // 2.0.0-0.beta.1 < 2.0.0-0.rc.1 < 2.0.0-1 and 2.0.0~beta.1 < 2.0.0~rc.1 < 2.0.0.
  assertEquals(linuxPackageVersions("2.0.0-beta.1"), {
    tauriVersion: "2.0.0",
    rpmRelease: "0.beta.1",
    debVersion: "2.0.0~beta.1",
  });
  assertEquals(linuxPackageVersions("2.1.0-rc.2"), {
    tauriVersion: "2.1.0",
    rpmRelease: "0.rc.2",
    debVersion: "2.1.0~rc.2",
  });
});

Deno.test("release versions must be plain semver", () => {
  assertThrows(() => linuxPackageVersions("2.0.0-alpha+abc"));
  assertThrows(() => linuxPackageVersions("v2"));
});
