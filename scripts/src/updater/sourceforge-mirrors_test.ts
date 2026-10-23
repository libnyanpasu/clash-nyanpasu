import assert from "node:assert/strict";
import { test } from "node:test";
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
      fileSize: name.endsWith(".sig") ? 20 : 123,
      sha256: String(index + 1).repeat(64),
    }])),
  });
}

test("SourceForge report parsing requires exact immutable HTTPS asset paths", () => {
  const parsed = manifest();
  assert.deepStrictEqual(parsed.remotePath, `releases/${tag}`);
  assert.deepStrictEqual(parsed.assets[binaryName].fileSize, 123);
  assert.throws(() =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        remotePath: "releases/latest",
      }), { message: /immutable build identity/ });
  assert.throws(() =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        assets: {
          ...parsed.assets,
          [binaryName]: {
            ...parsed.assets[binaryName],
            url: "https://example.com/stolen",
          },
        },
      }), { message: /Invalid SourceForge asset metadata for .*/ });
  assert.throws(() =>
      parseSourceforgeMirrorManifest({
        ...parsed,
        channel: "nightly",
        buildId: "latest",
        remotePath: "nightly/latest",
      }), { message: /full commit SHA/ });
});

test("release mirrors require the selected tag, matching assets, sizes, and signatures", () => {
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
  assert.deepStrictEqual(
    attachSourceforgeMirrors(platforms, releaseAssets, undefined, tag),
    platforms,
  );
  const mapped = attachSourceforgeMirrors(
    platforms,
    releaseAssets,
    mirror,
    tag,
  );
  assert.deepStrictEqual(mapped.win64, {
    ...platforms.win64,
    mirrors: { sourceforge: mirror.assets[binaryName].url },
    project,
    build_id: tag,
  });
  const proxy = mapUpdaterPlatformUrls(
    mapped,
    (url) => `https://proxy.example/${url}`,
  );
  assert.deepStrictEqual(proxy.win64.url, `https://proxy.example/${platforms.win64.url}`);
  assert.deepStrictEqual(proxy.win64.mirrors, mapped.win64.mirrors);
  assert.throws(() => attachSourceforgeMirrors(platforms, releaseAssets, mirror, "v2.3.0"), { message: /does not match selected release/ });
  assert.throws(() => attachSourceforgeMirrors(platforms, [releaseAssets[0]], mirror, tag), { message: /missing/ });
  assert.throws(() =>
      attachSourceforgeMirrors(
        platforms,
        [
          { ...releaseAssets[0], size: 124 },
          releaseAssets[1],
        ],
        mirror,
        tag,
      ), { message: /size differs/ });
});

test("release mirrors match product-name variants across storage feeds", () => {
  const names = [
    "Clash Nyanpasu_x64-setup.nsis.zip",
    "Clash Nyanpasu_x64-setup.nsis.zip.sig",
    "Clash Nyanpasu_amd64.AppImage.tar.gz",
    "Clash Nyanpasu_amd64.AppImage.tar.gz.sig",
    "Clash Nyanpasu.aarch64.app.tar.gz",
    "Clash Nyanpasu.aarch64.app.tar.gz.sig",
  ];
  const mirror = manifest(tag, "release", names);
  const platforms: Record<string, { url: string; signature: string }> = {};
  const releaseAssets = names.filter((name) => !name.endsWith(".sig")).flatMap(
    (mirrorName) => {
      const githubName = mirrorName.replace(
        /^Clash Nyanpasu(?=[_.-])/,
        "Clash.Nyanpasu",
      );
      const githubUrl =
        `https://github.com/owner/repo/releases/download/${tag}/${githubName}`;
      platforms[
        githubName.includes("aarch64.app")
          ? "darwin-aarch64"
          : githubName.includes("AppImage")
          ? "linux"
          : "win64"
      ] = { url: githubUrl, signature: "signed" };
      return [
        { name: githubName, browser_download_url: githubUrl, size: 123 },
        {
          name: `${githubName}.sig`,
          browser_download_url: `${githubUrl}.sig`,
          size: 20,
        },
      ];
    },
  );
  const mapped = attachSourceforgeMirrors(
    platforms,
    releaseAssets,
    mirror,
    tag,
  );

  for (const mirrorName of names.filter((name) => !name.endsWith(".sig"))) {
    const githubName = mirrorName.replace(
      /^Clash Nyanpasu(?=[_.-])/,
      "Clash.Nyanpasu",
    );
    const target = githubName.includes("aarch64.app")
      ? "darwin-aarch64"
      : githubName.includes("AppImage")
      ? "linux"
      : "win64";
    assert.deepStrictEqual(
      mapped[target].mirrors?.sourceforge,
      mirror.assets[mirrorName].url,
    );
  }
});

test("nightly mirrors bind the complete commit identity to the announced hash", () => {
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
  assert.deepStrictEqual(
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
  assert.throws(() =>
      validateNightlySourceforgeIdentity(
        mirror,
        "b".repeat(40),
        sha.slice(0, 7),
        "12345",
        "2",
      ), { message: /full commit/ });
  assert.throws(() =>
      validateNightlySourceforgeIdentity(
        mirror,
        sha,
        sha.slice(0, 7),
        "12345",
        "1",
      ), { message: /run, attempt, and full commit/ });
  assert.throws(() =>
      validateNightlySourceforgeIdentity(
        mirror,
        sha,
        "b".repeat(7),
        "12345",
        "2",
      ), { message: /announced version/ });
});

test("canonical release metadata is stable across asset object order", () => {
  const parsed = manifest();
  const reversed = parseSourceforgeMirrorManifest({
    ...parsed,
    assets: Object.fromEntries(Object.entries(parsed.assets).reverse()),
  });
  assert.deepStrictEqual(
    canonicalSourceforgeMirrorManifest(parsed),
    canonicalSourceforgeMirrorManifest(reversed),
  );
});

test("publishedAt accepts canonical UTC timestamps and survives persistence", () => {
  const parsed = parseSourceforgeMirrorManifest({
    ...manifest(),
    publishedAt: "2026-10-04T01:02:03.004Z",
  });
  assert.deepStrictEqual(parsed.publishedAt, "2026-10-04T01:02:03.004Z");
  assert.deepStrictEqual(
    JSON.parse(canonicalSourceforgeMirrorManifest(parsed)).publishedAt,
    parsed.publishedAt,
  );

  const laterBatch = parseSourceforgeMirrorManifest({
    ...parsed,
    publishedAt: "2026-10-04T02:02:03.004Z",
  });
  assert.deepStrictEqual(sourceforgeMirrorAssetsEqual(parsed, laterBatch), true);
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
  assert.deepStrictEqual(sourceforgeMirrorAssetsEqual(parsed, changedAsset), false);

  for (
    const publishedAt of [
      "2026-10-04T01:02:03Z",
      "2026-10-04T01:02:03.004+00:00",
      "2026-02-30T01:02:03.004Z",
      "yesterday",
    ]
  ) {
    assert.throws(() => parseSourceforgeMirrorManifest({ ...manifest(), publishedAt }), { message: /Invalid SourceForge publication timestamp/ });
  }

  assert.deepStrictEqual(manifest().publishedAt, undefined);
  assert.deepStrictEqual(
    JSON.parse(canonicalSourceforgeMirrorManifest(manifest())).publishedAt,
    undefined,
  );
});
