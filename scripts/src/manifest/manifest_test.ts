import assert from "node:assert/strict";
import { test } from "node:test";
import { type GitHubRelease, resolveMeowAlphaRelease } from "./manifest.ts";

const alphaRelease: GitHubRelease = {
  tag_name: "Prerelease-Alpha",
  prerelease: true,
  assets: [
    { name: "meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz" },
    { name: "meow-alpha-3c27aca-aarch64-pc-windows-msvc.zip" },
    { name: "meow-alpha-3c27aca-aarch64-unknown-linux-musl.tar.gz" },
    { name: "meow-alpha-3c27aca-x86_64-apple-darwin.tar.gz" },
    { name: "meow-alpha-3c27aca-x86_64-pc-windows-msvc.zip" },
    { name: "meow-alpha-3c27aca-x86_64-unknown-linux-musl.tar.gz" },
  ],
};

test("resolve Meow Alpha from all official release assets", () => {
  const result = resolveMeowAlphaRelease(alphaRelease);
  assert.deepStrictEqual(result.name, "meow_alpha");
  assert.deepStrictEqual(result.version, "alpha-3c27aca");
  assert.deepStrictEqual(
    result.archMapping["darwin-arm64"],
    "meow-{}-aarch64-apple-darwin.tar.gz",
  );
  assert.deepStrictEqual(
    result.archMapping["linux-amd64"],
    "meow-{}-x86_64-unknown-linux-musl.tar.gz",
  );
});

test("reject incomplete or mixed-commit Meow Alpha release assets", () => {
  assert.throws(() =>
    resolveMeowAlphaRelease({
      ...alphaRelease,
      assets: alphaRelease.assets.slice(1),
    }));

  const mixedRelease = structuredClone(alphaRelease);
  mixedRelease.assets[0].name = mixedRelease.assets[0].name.replace(
    "3c27aca",
    "deadbee",
  );
  assert.throws(() => resolveMeowAlphaRelease(mixedRelease));
});
