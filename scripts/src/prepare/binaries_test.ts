import { assertEquals } from "jsr:@std/assert@1";
import { createBinaryResolvers } from "./binaries.ts";
import type { ArchMapping, SupportedArch, VersionManifest } from "./types.ts";

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

function versionManifest(): VersionManifest {
  return {
    manifest_version: 1,
    latest: {
      mihomo: "mihomo-v1",
      mihomo_alpha: "alpha-v1",
      clash_rs: "clash-rs-v1",
      clash_premium: "2026-10-01",
      clash_rs_alpha: "clash-rs-alpha-v1",
      meow: "meow-v1",
    },
    arch_template: {
      mihomo: mapping("mihomo"),
      mihomo_alpha: mapping("mihomo-alpha"),
      clash_rs: mapping("clash-rs"),
      clash_premium: mapping("clash"),
      clash_rs_alpha: mapping("clash-rs-alpha"),
      meow: mapping("meow"),
    },
    updated_at: "2026-10-01",
  };
}

Deno.test("stable binary resolvers map assets for all supported platforms", () => {
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
    assertEquals(clash.targetFile, `clash-${host}${executableSuffix}`);
    assertEquals(clash.exeFile, `clash-${label}-2026-10-01${executableSuffix}`);
    assertEquals(
      clash.downloadURL,
      "https://github.com/zhongfly/Clash-premium-backup/releases/download/2026-10-01/clash-" +
        `${label}-2026-10-01`,
    );

    const mihomo = binary.mihomo();
    assertEquals(mihomo.targetFile, `mihomo-${host}${executableSuffix}`);
    assertEquals(
      mihomo.exeFile,
      `mihomo-${label}-mihomo-v1${executableSuffix}`,
    );
    assertEquals(
      mihomo.downloadURL,
      `https://github.com/MetaCubeX/mihomo/releases/download/mihomo-v1/mihomo-${label}-mihomo-v1`,
    );

    const clashRs = binary.clashRs();
    assertEquals(clashRs.targetFile, `clash-rs-${host}${executableSuffix}`);
    assertEquals(clashRs.exeFile, `clash-rs-${label}-clash-rs-v1`);
    assertEquals(
      clashRs.downloadURL,
      `https://github.com/ibigbug/clash-rs/releases/download/clash-rs-v1/clash-rs-${label}-clash-rs-v1`,
    );

    const meow = binary.meow();
    assertEquals(meow.targetFile, `meow-${host}${executableSuffix}`);
    assertEquals(meow.exeFile, `meow-${label}-meow-v1`);
    assertEquals(
      meow.downloadURL,
      `https://github.com/madeye/meow-rs/releases/download/meow-v1/meow-${label}-meow-v1`,
    );
  }
});

Deno.test("nyanpasu-service version and asset derive from its Cargo manifest", async () => {
  const workspaceRoot = await Deno.makeTempDir();
  const manifestPath =
    `${workspaceRoot}/backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml`;
  await Deno.mkdir(manifestPath.replace(/\/Cargo\.toml$/, ""), {
    recursive: true,
  });
  await Deno.writeTextFile(
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
    assertEquals(await binary.nyanpasuService(), {
      name: "nyanpasu-service",
      version: "v2.3.4-rc.1",
      targetFile: "nyanpasu-service-x86_64-pc-windows-msvc.exe",
      exeFile: "nyanpasu-service.exe",
      tmpFile: "nyanpasu-service-v2.3.4-rc.1-x86_64-pc-windows-msvc.zip",
      downloadURL:
        "https://github.com/libnyanpasu/nyanpasu-runtime/releases/download/v2.3.4-rc.1/nyanpasu-service-x86_64-pc-windows-msvc.zip",
    });
  } finally {
    await Deno.remove(workspaceRoot, { recursive: true });
  }
});
