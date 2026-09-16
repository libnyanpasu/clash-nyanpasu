import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  collectUpdaterPlatforms,
  type ReleaseAsset,
  UPDATER_TARGETS,
} from "./updater-platforms.ts";

function asset(name: string): ReleaseAsset {
  return { name, browser_download_url: `https://example.com/${name}` };
}

Deno.test("one manifest contains all platforms and both Windows variants with paired signatures", async () => {
  const archives = [
    "Clash.Nyanpasu_2.0.0_x64-setup.nsis.zip",
    "Clash.Nyanpasu_2.0.0_fixed-webview-x64-setup.nsis.zip",
    "Clash.Nyanpasu_2.0.0_arm64-setup.nsis.zip",
    "Clash.Nyanpasu_2.0.0_fixed-webview-arm64-setup.nsis.zip",
    "Clash.Nyanpasu_x86_64.app.tar.gz",
    "Clash.Nyanpasu_aarch64.app.tar.gz",
    "Clash.Nyanpasu_amd64.AppImage.tar.gz",
  ];
  const assets = archives.flatMap((
    name,
  ) => [asset(`${name}.sig`), asset(name)]);
  assets.push(
    asset("Clash.Nyanpasu_fixed-webview_portable.zip"),
    asset("latest.json"),
  );
  const read: string[] = [];
  const platforms = await collectUpdaterPlatforms(assets, (url) => {
    read.push(url);
    return Promise.resolve(`signature:${url}\n`);
  });
  assertEquals(Object.keys(platforms).sort(), [...UPDATER_TARGETS].sort());
  const expected: Record<string, number> = {
    win64: 0,
    "windows-x86_64": 0,
    "windows-x86_64-fixed-webview": 1,
    "windows-aarch64": 2,
    "windows-aarch64-fixed-webview": 3,
    darwin: 4,
    "darwin-intel": 4,
    "darwin-x86_64": 4,
    "darwin-aarch64": 5,
    linux: 6,
    "linux-x86_64": 6,
  };
  for (const [target, index] of Object.entries(expected)) {
    const url = `https://example.com/${archives[index]}`;
    assertEquals(platforms[target], { url, signature: `signature:${url}.sig` });
  }
  assertEquals(read.length, archives.length);
});

Deno.test("fixed-only and standard-only assets do not masquerade as the other variant", async () => {
  for (
    const [name, targets] of [
      ["Clash.Nyanpasu_fixed-webview-x64.nsis.zip", [
        "windows-x86_64-fixed-webview",
      ]],
      ["Clash.Nyanpasu_x64.nsis.zip", ["win64", "windows-x86_64"]],
    ] as const
  ) {
    const platforms = await collectUpdaterPlatforms([
      asset(name),
      asset(`${name}.sig`),
    ], () => Promise.resolve("signature"));
    assertEquals(Object.keys(platforms).sort(), [...targets].sort());
  }
});

Deno.test("incomplete or ambiguous artifacts fail manifest generation", async () => {
  const name = "Clash.Nyanpasu_x64.nsis.zip";
  await assertRejects(
    () => collectUpdaterPlatforms([asset(name)], () => Promise.resolve("sig")),
    Error,
    "Missing signature",
  );
  await assertRejects(
    () =>
      collectUpdaterPlatforms(
        [asset(name), asset(`${name}.sig`)],
        () => Promise.resolve(""),
      ),
    Error,
    "Empty signature",
  );
  await assertRejects(
    () =>
      collectUpdaterPlatforms(
        [asset(name), asset(`${name}.sig`)],
        () => Promise.reject(new Error("download failed")),
      ),
    Error,
    "download failed",
  );
  const duplicate = "Clash.Nyanpasu_old_x64.nsis.zip";
  await assertRejects(
    () =>
      collectUpdaterPlatforms([
        asset(name),
        asset(`${name}.sig`),
        asset(duplicate),
        asset(`${duplicate}.sig`),
      ], () => Promise.resolve("sig")),
    Error,
    "Multiple updater archives",
  );
});
