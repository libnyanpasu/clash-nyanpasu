import assert from "node:assert/strict";
import { test } from "node:test";
import { createBinaryResolvers } from "./binaries.ts";
import type { ArchMapping, SupportedArch, VersionManifest } from "./types.ts";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const platforms: Array<{
  platform: string;
  arch: string;
  label: SupportedArch;
  host: string;
  executableSuffix: string;
}> = [
  {
    platform: "win32",
    arch: "x64",
    label: "windows-x86_64",
    host: "x86_64-pc-windows-msvc",
    executableSuffix: ".exe",
  },
  {
    platform: "win32",
    arch: "arm64",
    label: "windows-arm64",
    host: "aarch64-pc-windows-msvc",
    executableSuffix: ".exe",
  },
  {
    platform: "linux",
    arch: "arm64",
    label: "linux-aarch64",
    host: "aarch64-unknown-linux-gnu",
    executableSuffix: "",
  },
  {
    platform: "linux",
    arch: "x64",
    label: "linux-amd64",
    host: "x86_64-unknown-linux-gnu",
    executableSuffix: "",
  },
  {
    platform: "darwin",
    arch: "arm64",
    label: "darwin-arm64",
    host: "aarch64-apple-darwin",
    executableSuffix: "",
  },
  {
    platform: "darwin",
    arch: "x64",
    label: "darwin-x64",
    host: "x86_64-apple-darwin",
    executableSuffix: "",
  },
];

function mapping(prefix: string): ArchMapping {
  return Object.fromEntries(
    platforms.map(({ label }) => [label, `${prefix}-${label}-{}`]),
  ) as ArchMapping;
}

function meowMapping(): ArchMapping {
  const targets: Record<SupportedArch, string> = {
    "windows-x86_64": "x86_64-pc-windows-msvc.zip",
    "windows-arm64": "aarch64-pc-windows-msvc.zip",
    "linux-aarch64": "aarch64-unknown-linux-musl.tar.gz",
    "linux-amd64": "x86_64-unknown-linux-musl.tar.gz",
    "darwin-arm64": "aarch64-apple-darwin.tar.gz",
    "darwin-x64": "x86_64-apple-darwin.tar.gz",
  };
  return Object.fromEntries(
    Object.entries(targets).map((
      [arch, target],
    ) => [arch, `meow-{}-${target}`]),
  ) as ArchMapping;
}

function versionManifest(): VersionManifest {
  return {
    manifest_version: 1,
    latest: {
      mihomo: "mihomo-v1",
      mihomo_alpha: "alpha-v1",
      clash_rs: "clash-rs-v1",
      clash_premium: "2026-10-01",
      clash_rs_alpha: "clash-rs-alpha-v1",
      meow: "v0.22.0",
      meow_alpha: "alpha-v1",
    },
    arch_template: {
      mihomo: mapping("mihomo"),
      mihomo_alpha: mapping("mihomo-alpha"),
      clash_rs: mapping("clash-rs"),
      clash_premium: mapping("clash"),
      clash_rs_alpha: mapping("clash-rs-alpha"),
      meow: meowMapping(),
      meow_alpha: meowMapping(),
    },
    updated_at: "2026-10-01",
  };
}

test("stable binary resolvers map assets for all supported platforms", () => {
  const manifest = versionManifest();
  for (const { platform, arch, label, host, executableSuffix } of platforms) {
    const binary = createBinaryResolvers({
      versionManifest: manifest,
      sidecarHost: host,
      platform,
      arch,
      workspaceRoot: "/workspace",
    });
    const clash = binary.clash();
    assert.deepStrictEqual(clash.targetFile, `clash-${host}${executableSuffix}`);
    assert.deepStrictEqual(clash.exeFile, `clash-${label}-2026-10-01${executableSuffix}`);
    assert.deepStrictEqual(
      clash.downloadURL,
      "https://github.com/zhongfly/Clash-premium-backup/releases/download/2026-10-01/clash-" +
        `${label}-2026-10-01`,
    );

    const mihomo = binary.mihomo();
    assert.deepStrictEqual(mihomo.targetFile, `mihomo-${host}${executableSuffix}`);
    assert.deepStrictEqual(
      mihomo.exeFile,
      `mihomo-${label}-mihomo-v1${executableSuffix}`,
    );
    assert.deepStrictEqual(
      mihomo.downloadURL,
      `https://github.com/MetaCubeX/mihomo/releases/download/mihomo-v1/mihomo-${label}-mihomo-v1`,
    );

    const clashRs = binary.clashRs();
    assert.deepStrictEqual(clashRs.targetFile, `clash-rs-${host}${executableSuffix}`);
    assert.deepStrictEqual(clashRs.exeFile, `clash-rs-${label}-clash-rs-v1`);
    assert.deepStrictEqual(
      clashRs.downloadURL,
      `https://github.com/ibigbug/clash-rs/releases/download/clash-rs-v1/clash-rs-${label}-clash-rs-v1`,
    );

    const meow = binary.meow();
    assert.deepStrictEqual(meow.targetFile, `meow-${host}${executableSuffix}`);
    assert.deepStrictEqual(
      meow.exeFile,
      platform === "win32"
        ? "meow.exe"
        : `meow-v0.22.0-${
          host.replace("-unknown-linux-gnu", "-unknown-linux-musl")
        }/meow`,
    );
    const stableAsset = meowMapping()[label].replace("{}", "v0.22.0");
    assert.deepStrictEqual(
      meow.downloadURL,
      `https://github.com/meow-rs/meow-rs/releases/download/v0.22.0/${stableAsset}`,
    );

    const meowAlpha = binary.meowAlpha();
    assert.deepStrictEqual(meowAlpha.name, "meow-alpha");
    assert.deepStrictEqual(meowAlpha.version, "alpha-v1");
    assert.deepStrictEqual(meowAlpha.targetFile, `meow-alpha-${host}${executableSuffix}`);
    assert.deepStrictEqual(
      meowAlpha.exeFile,
      platform === "win32"
        ? "meow.exe"
        : `meow-alpha-v1-${
          host.replace("-unknown-linux-gnu", "-unknown-linux-musl")
        }/meow`,
    );
    const alphaAsset = meowMapping()[label].replace("{}", "alpha-v1");
    assert.deepStrictEqual(meowAlpha.tmpFile, alphaAsset);
    assert.deepStrictEqual(
      meowAlpha.downloadURL,
      `https://github.com/meow-rs/meow-rs/releases/download/Prerelease-Alpha/${alphaAsset}`,
    );
  }
});

test("nyanpasu-service version and asset derive from its Cargo manifest", async () => {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  const manifestPath =
    `${workspaceRoot}/backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml`;
  await mkdir(manifestPath.replace(/\/Cargo\.toml$/, ""), {
    recursive: true,
  });
  await writeFile(
    manifestPath,
    '[package]\nname = "nyanpasu-service"\nversion = "2.3.4-rc.1"\n',
  );

  try {
    const binary = createBinaryResolvers({
      versionManifest: versionManifest(),
      sidecarHost: "x86_64-pc-windows-msvc",
      platform: "win32",
      arch: "x64",
      workspaceRoot,
    });
    assert.deepStrictEqual(await binary.nyanpasuService(), {
      name: "nyanpasu-service",
      version: "v2.3.4-rc.1",
      targetFile: "nyanpasu-service-x86_64-pc-windows-msvc.exe",
      exeFile: "nyanpasu-service.exe",
      tmpFile: "nyanpasu-service-v2.3.4-rc.1-x86_64-pc-windows-msvc.zip",
      downloadURL:
        "https://github.com/libnyanpasu/nyanpasu-runtime/releases/download/v2.3.4-rc.1/nyanpasu-service-x86_64-pc-windows-msvc.zip",
    });
  } finally {
    await rm(workspaceRoot, { recursive: true });
  }
});

test("Meow Alpha refresh resolves the current rolling tag asset", async () => {
  const originalFetch = globalThis.fetch;
  const requested: string[] = [];
  globalThis.fetch = (input) => {
    const url = String(input);
    requested.push(url);
    return Promise.resolve(
      new Response("meow-alpha-1fd134a-x86_64-apple-darwin.tar.gz"),
    );
  };

  try {
    const binary = createBinaryResolvers({
      versionManifest: versionManifest(),
      sidecarHost: "x86_64-apple-darwin",
      platform: "darwin",
      arch: "x64",
      workspaceRoot: "/workspace",
    });
    const pinned = binary.meowAlpha();
    assert.deepStrictEqual(pinned.version, "alpha-v1");
    assert.deepStrictEqual(typeof pinned.refresh, "function");

    const refreshed = await pinned.refresh!();
    assert.deepStrictEqual(requested, [
      "https://github.com/meow-rs/meow-rs/releases/expanded_assets/Prerelease-Alpha",
    ]);
    assert.deepStrictEqual(refreshed.version, "alpha-1fd134a");
    assert.deepStrictEqual(
      refreshed.tmpFile,
      "meow-alpha-1fd134a-x86_64-apple-darwin.tar.gz",
    );
    assert.deepStrictEqual(
      refreshed.downloadURL,
      "https://github.com/meow-rs/meow-rs/releases/download/Prerelease-Alpha/" +
        "meow-alpha-1fd134a-x86_64-apple-darwin.tar.gz",
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("Mihomo Alpha refresh re-resolves the live version", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (input) =>
    String(input).endsWith("/version.txt")
      ? Promise.resolve(new Response("alpha-bc76c13"))
      : Promise.resolve(
        new Response("mihomo-linux-amd64-v2-alpha-bc76c13.gz"),
      );

  try {
    const binary = createBinaryResolvers({
      versionManifest: versionManifest(),
      sidecarHost: "x86_64-unknown-linux-gnu",
      platform: "linux",
      arch: "x64",
      workspaceRoot: "/workspace",
    });
    const pinned = await binary.mihomoAlpha();
    assert.deepStrictEqual(pinned.version, "alpha-bc76c13");
    assert.deepStrictEqual(typeof pinned.refresh, "function");

    const refreshed = await pinned.refresh!();
    assert.deepStrictEqual(refreshed.version, "alpha-bc76c13");
    assert.deepStrictEqual(refreshed.tmpFile, "mihomo-alpha-linux-amd64-alpha-bc76c13");
    assert.deepStrictEqual(
      refreshed.downloadURL,
      "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/" +
        "mihomo-alpha-linux-amd64-alpha-bc76c13",
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
