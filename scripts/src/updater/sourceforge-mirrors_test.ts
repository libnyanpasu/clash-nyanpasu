import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import {
  attachNightlySourceforgeMirrors,
  attachSourceforgeMirrors,
  canonicalSourceforgeMirrorManifest,
  mapUpdaterPlatformUrls,
  parseSourceforgeMirrorManifest,
  sourceforgeMirrorAssetsEqual,
  type SourceforgeMirrorManifest,
  validateNightlySourceforgeIdentity,
} from "./sourceforge-mirrors.ts";

const project = "nyanpasu";
const tag = "v2.4.0-beta.1";
const sha = "a".repeat(40);
const binaryName = "Clash.Nyanpasu_x64.nsis.zip";
const signatureName = `${binaryName}.sig`;

function manifest(
  buildId = tag,
  channel: "release" | "nightly" = "release",
  names = [binaryName, signatureName],
): SourceforgeMirrorManifest {
  const remotePath = `${
    channel === "nightly" ? "nightly" : "releases"
  }/${buildId}`;
  return parseSourceforgeMirrorManifest({
    schemaVersion: 1,
    project,
    channel,
    buildId,
    remotePath,
    assets: Object.fromEntries(names.map((name, index) => [name, {
      url:
        `https://downloads.sourceforge.net/project/${project}/${remotePath}/${
          encodeURIComponent(name)
        }`,
      fileSize: index === 0 ? 123 : 20,
      sha256: String(index + 1).repeat(64),
    }])),
  });
}

Deno.test("SourceForge report parsing requires exact immutable HTTPS asset paths", () => {
  const parsed = manifest();
  assertEquals(parsed.remotePath, `releases/${tag}`);
  assertEquals(parsed.assets[binaryName].fileSize, 123);
  assertThrows(
    () =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        remotePath: "releases/latest",
      }),
    Error,
    "immutable build identity",
  );
  assertThrows(
    () =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        assets: {
          ...parsed.assets,
          [binaryName]: {
            ...parsed.assets[binaryName],
            url: "https://example.com/stolen",
          },
        },
      }),
    Error,
    `Invalid SourceForge asset metadata for ${binaryName}`,
  );
  assertThrows(
    () =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        channel: "nightly",
        buildId: "latest",
        remotePath: "nightly/latest",
      }),
    Error,
    "full commit SHA",
  );
});

Deno.test("release mirrors require the selected tag, matching assets, sizes, and signatures", () => {
  const mirror = manifest();
  const platforms = {
    win64: {
      url:
        `https://github.com/owner/repo/releases/download/${tag}/${binaryName}`,
      signature: "signed",
    },
  };
  const releaseAssets = [
    { name: binaryName, browser_download_url: platforms.win64.url, size: 123 },
    {
      name: signatureName,
      browser_download_url: `${platforms.win64.url}.sig`,
      size: 20,
    },
  ];
  assertEquals(
    attachSourceforgeMirrors(platforms, releaseAssets, undefined, tag),
    platforms,
  );
  const mapped = attachSourceforgeMirrors(
    platforms,
    releaseAssets,
    mirror,
    tag,
  );
  assertEquals(mapped.win64, {
    ...platforms.win64,
    mirrors: { sourceforge: mirror.assets[binaryName].url },
    project,
    build_id: tag,
  });
  const proxy = mapUpdaterPlatformUrls(
    mapped,
    (url) => `https://proxy.example/${url}`,
  );
  assertEquals(proxy.win64.url, `https://proxy.example/${platforms.win64.url}`);
  assertEquals(proxy.win64.mirrors, mapped.win64.mirrors);
  assertThrows(
    () => attachSourceforgeMirrors(platforms, releaseAssets, mirror, "v2.3.0"),
    Error,
    "does not match selected release",
  );
  assertThrows(
    () => attachSourceforgeMirrors(platforms, [releaseAssets[0]], mirror, tag),
    Error,
    "missing",
  );
  assertThrows(
    () =>
      attachSourceforgeMirrors(
        platforms,
        [
          { ...releaseAssets[0], size: 124 },
          releaseAssets[1],
        ],
        mirror,
        tag,
      ),
    Error,
    "size differs",
  );
});

Deno.test("nightly mirrors bind the complete commit identity to the announced hash", () => {
  const buildId = `12345-2-${sha}`;
  const mirror = manifest(buildId, "nightly");
  validateNightlySourceforgeIdentity(
    mirror,
    sha,
    sha.slice(0, 7),
    "12345",
    "2",
  );
  const platforms = {
    win64: {
      url:
        `https://github.com/owner/repo/releases/download/pre-release/${binaryName}`,
      signature: "signed",
    },
  };
  const releaseAssets = [
    { name: binaryName, browser_download_url: platforms.win64.url, size: 123 },
    {
      name: signatureName,
      browser_download_url: `${platforms.win64.url}.sig`,
      size: 20,
    },
  ];
  assertEquals(
    attachNightlySourceforgeMirrors(
      platforms,
      releaseAssets,
      mirror,
      sha,
      sha.slice(0, 7),
      "12345",
      "2",
    ).win64.build_id,
    buildId,
  );
  assertThrows(
    () =>
      validateNightlySourceforgeIdentity(
        mirror,
        "b".repeat(40),
        sha.slice(0, 7),
        "12345",
        "2",
      ),
    Error,
    "full commit",
  );
  assertThrows(
    () =>
      validateNightlySourceforgeIdentity(
        mirror,
        sha,
        sha.slice(0, 7),
        "12345",
        "1",
      ),
    Error,
    "run, attempt, and full commit",
  );
  assertThrows(
    () =>
      validateNightlySourceforgeIdentity(
        mirror,
        sha,
        "b".repeat(7),
        "12345",
        "2",
      ),
    Error,
    "announced version",
  );
});

Deno.test("canonical release metadata is stable across asset object order", () => {
  const parsed = manifest();
  const reversed = parseSourceforgeMirrorManifest({
    ...parsed,
    assets: Object.fromEntries(Object.entries(parsed.assets).reverse()),
  });
  assertEquals(
    canonicalSourceforgeMirrorManifest(parsed),
    canonicalSourceforgeMirrorManifest(reversed),
  );
});

Deno.test("publishedAt accepts canonical UTC timestamps and survives persistence", () => {
  const parsed = parseSourceforgeMirrorManifest({
    ...manifest(),
    publishedAt: "2026-10-04T01:02:03.004Z",
  });
  assertEquals(parsed.publishedAt, "2026-10-04T01:02:03.004Z");
  assertEquals(
    JSON.parse(canonicalSourceforgeMirrorManifest(parsed)).publishedAt,
    parsed.publishedAt,
  );

  const laterBatch = parseSourceforgeMirrorManifest({
    ...parsed,
    publishedAt: "2026-10-04T02:02:03.004Z",
  });
  assertEquals(sourceforgeMirrorAssetsEqual(parsed, laterBatch), true);
  const changedAsset = parseSourceforgeMirrorManifest({
    ...laterBatch,
    assets: {
      ...laterBatch.assets,
      [binaryName]: {
        ...laterBatch.assets[binaryName],
        sha256: "f".repeat(64),
      },
    },
  });
  assertEquals(sourceforgeMirrorAssetsEqual(parsed, changedAsset), false);

  for (
    const publishedAt of [
      "2026-10-04T01:02:03Z",
      "2026-10-04T01:02:03.004+00:00",
      "2026-02-30T01:02:03.004Z",
      "yesterday",
    ]
  ) {
    assertThrows(
      () => parseSourceforgeMirrorManifest({ ...manifest(), publishedAt }),
      Error,
      "Invalid SourceForge publication timestamp",
    );
  }

  assertEquals(manifest().publishedAt, undefined);
  assertEquals(
    JSON.parse(canonicalSourceforgeMirrorManifest(manifest())).publishedAt,
    undefined,
  );
});
