import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { type GitHubRelease, resolveMeowAlphaRelease } from "./manifest.ts";

const alphaRelease: GitHubRelease = {
  tag_name: "Prerelease-Alpha",
  prerelease: true,
  assets: [
    {
      name: "meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz",
      digest:
        "sha256:aaa0c4d9b8693de580681c839b5e9cdf785ecd164ffec1555056f83bb6541a37",
    },
    {
      name: "meow-alpha-3c27aca-aarch64-pc-windows-msvc.zip",
      digest:
        "sha256:8a279b9a53f50c6754a119e8930348b194f111a2e3f3d29469e1a24ba3ba0481",
    },
    {
      name: "meow-alpha-3c27aca-aarch64-unknown-linux-musl.tar.gz",
      digest:
        "sha256:fbd2fef18cb594f2d6e3a885d9809f4950f08b6e99c560da0844e195c7872112",
    },
    {
      name: "meow-alpha-3c27aca-x86_64-apple-darwin.tar.gz",
      digest:
        "sha256:facecb84b7b55036c2881af414724dcb9f1785adaef48b3d504c1c41c9f88348",
    },
    {
      name: "meow-alpha-3c27aca-x86_64-pc-windows-msvc.zip",
      digest:
        "sha256:a2c49300ae636062068d3ddf567b541fd6bf1c9c577682cf4877d74ab95edaa1",
    },
    {
      name: "meow-alpha-3c27aca-x86_64-unknown-linux-musl.tar.gz",
      digest:
        "sha256:1df24d0bb173fae2a0854c76d254b4b4251a35b3cd40ec8622fefe6bfe72bf12",
    },
  ],
};

Deno.test("resolve Meow Alpha from all official release assets and digests", () => {
  const result = resolveMeowAlphaRelease(alphaRelease);
  assertEquals(result.name, "meow_alpha");
  assertEquals(result.version, "alpha-3c27aca");
  assertEquals(
    result.archMapping["darwin-arm64"],
    "meow-{}-aarch64-apple-darwin.tar.gz",
  );
  assertEquals(
    result.sha256ByArch["linux-amd64"],
    "1df24d0bb173fae2a0854c76d254b4b4251a35b3cd40ec8622fefe6bfe72bf12",
  );
});

Deno.test("reject incomplete or mixed-commit Meow Alpha release assets", () => {
  assertThrows(() =>
    resolveMeowAlphaRelease({
      ...alphaRelease,
      assets: alphaRelease.assets.slice(1),
    })
  );

  const mixedRelease = structuredClone(alphaRelease);
  mixedRelease.assets[0].name = mixedRelease.assets[0].name.replace(
    "3c27aca",
    "deadbee",
  );
  assertThrows(() => resolveMeowAlphaRelease(mixedRelease));
});

Deno.test("reject Meow Alpha release assets without GitHub digests", () => {
  const releaseWithoutDigest = structuredClone(alphaRelease);
  delete releaseWithoutDigest.assets[0].digest;
  assertThrows(() => resolveMeowAlphaRelease(releaseWithoutDigest));
});
