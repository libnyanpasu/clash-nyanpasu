import { getOctokit } from "@actions/github";
import fs from "fs-extra";
import path from "node:path";
import { MANIFEST_DIR } from "./utils/env";
import { consola } from "./utils/logger";

const GITHUB_TOKEN = process.env.GITHUB_TOKEN || "";

const MANIFEST_VERSION_PATH = path.join(MANIFEST_DIR, "version.json");

export enum SupportedArch {
  // blocked by clash-rs
  // WindowsX86 = "windows-x86",
  WindowsX86_64 = "windows-x86_64",
  // blocked by clash-rs#212
  // WindowsArm64 = "windows-arm64",
  LinuxAarch64 = "linux-aarch64",
  LinuxAmd64 = "linux-amd64",
  DarwinArm64 = "darwin-arm64",
  DarwinX64 = "darwin-x64",
}

export enum SupportedCore {
  Mihomo = "mihomo",
  MihomoAlpha = "mihomo_alpha",
  ClashRs = "clash_rs",
  ClashPremium = "clash_premium",
}

export type ArchMapping = { [key in SupportedArch]: string };

export interface ManifestVersion {
  manifest_version: number;
  latest: { [K in SupportedCore]: string };
  arch_template: { [K in SupportedCore]: ArchMapping };
  updated_at: string; // ISO 8601
}

const MANIFEST_VERSION = 1;

let previousManifest: ManifestVersion | null = null;
const getPreviousManifest = async (): Promise<void> => {
  const isExist = await fs.pathExists(MANIFEST_VERSION_PATH);
  if (!isExist) {
    previousManifest = null;
    return;
  }
  previousManifest = (await fs.readJSON(
    MANIFEST_VERSION_PATH,
  )) as ManifestVersion;
};

// resolvers block
type LatestVersionResolver = () => Promise<{
  name: string;
  version: string;
  archMapping: ArchMapping;
}>;

const resolveMihomo: LatestVersionResolver = async () => {
  const octokit = getOctokit(GITHUB_TOKEN);
  const latestRelease = await octokit.rest.repos.getLatestRelease({
    owner: "MetaCubeX",
    repo: "mihomo",
  });
  consola.debug(`mihomo latest release: ${latestRelease.data.tag_name}`);

  const archMapping: ArchMapping = {
    // [SupportedArch.WindowsX86]: "mihomo-windows-386-{}.zip",
    [SupportedArch.WindowsX86_64]: "mihomo-windows-amd64-compatible-{}.zip",
    // [SupportedArch.WindowsAarch64]: "mihomo-windows-arm64-{}.zip",
    [SupportedArch.LinuxAarch64]: "mihomo-linux-arm64-{}.gz",
    [SupportedArch.LinuxAmd64]: "mihomo-linux-amd64-compatible-{}.gz",
    [SupportedArch.DarwinArm64]: "mihomo-darwin-arm64-{}.gz",
    [SupportedArch.DarwinX64]: "mihomo-darwin-amd64-compatible-{}.gz",
  } satisfies ArchMapping;
  return {
    name: "mihomo",
    version: latestRelease.data.tag_name,
    archMapping,
  };
};

const resolveMihomoAlpha: LatestVersionResolver = async () => {
  const resp = await fetch(
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
  );
  const alphaReleaseHash = (await resp.text()).trim();
  consola.debug(`mihomo alpha release: ${alphaReleaseHash}`);

  const archMapping: ArchMapping = {
    // [SupportedArch.WindowsX86]: "mihomo-windows-386-{}.zip",
    [SupportedArch.WindowsX86_64]: "mihomo-windows-amd64-compatible-{}.zip",
    // [SupportedArch.WindowsAarch64]: "mihomo-windows-arm64-{}.zip",
    [SupportedArch.LinuxAarch64]: "mihomo-linux-arm64-{}.gz",
    [SupportedArch.LinuxAmd64]: "mihomo-linux-amd64-compatible-{}.gz",
    [SupportedArch.DarwinArm64]: "mihomo-darwin-arm64-{}.gz",
    [SupportedArch.DarwinX64]: "mihomo-darwin-amd64-compatible-{}.gz",
  } satisfies ArchMapping;
  return {
    name: "mihomo_alpha",
    version: alphaReleaseHash,
    archMapping,
  };
};

const resolveClashRs: LatestVersionResolver = async () => {
  const octokit = getOctokit(GITHUB_TOKEN);
  const latestRelease = await octokit.rest.repos.getLatestRelease({
    owner: "Watfaq",
    repo: "clash-rs",
  });
  consola.debug(`clash-rs latest release: ${latestRelease.data.tag_name}`);

  const archMapping: ArchMapping = {
    // [SupportedArch.WindowsX86]: "mihomo-windows-386-alpha-{}.zip",
    [SupportedArch.WindowsX86_64]: "clash-x86_64-pc-windows-msvc.exe",
    // [SupportedArch.WindowsAarch64]: "mihomo-windows-arm64-alpha-{}.zip",
    [SupportedArch.LinuxAarch64]: "clash-aarch64-unknown-linux-gnu-static-crt",
    [SupportedArch.LinuxAmd64]: "clash-x86_64-unknown-linux-gnu-static-crt",
    [SupportedArch.DarwinArm64]: "clash-aarch64-apple-darwin",
    [SupportedArch.DarwinX64]: "clash-x86_64-apple-darwin",
  } satisfies ArchMapping;
  return {
    name: "clash_rs",
    version: latestRelease.data.tag_name,
    archMapping,
  };
};

const resolveClashPremium: LatestVersionResolver = async () => {
  const octokit = getOctokit(GITHUB_TOKEN);
  const latestRelease = await octokit.rest.repos.getLatestRelease({
    owner: "zhongfly",
    repo: "Clash-premium-backup",
  });
  consola.debug(`clash-premium latest release: ${latestRelease.data.tag_name}`);

  const archMapping: ArchMapping = {
    // [SupportedArch.WindowsX86]: "clash-windows-386-n{}.zip",
    [SupportedArch.WindowsX86_64]: "clash-windows-amd64-n{}.zip",
    // [SupportedArch.WindowsAarch64]: "clash-windows-arm64-n{}.zip",
    [SupportedArch.LinuxAarch64]: "clash-linux-arm64-n{}.gz",
    [SupportedArch.LinuxAmd64]: "clash-linux-amd64-n{}.gz",
    [SupportedArch.DarwinArm64]: "clash-darwin-arm64-n{}.gz",
    [SupportedArch.DarwinX64]: "clash-darwin-amd64-n{}.gz",
  } satisfies ArchMapping;
  return {
    name: "clash_premium",
    version: latestRelease.data.tag_name,
    archMapping,
  };
};

async function main() {
  if (!GITHUB_TOKEN) {
    consola.fatal("GITHUB_TOKEN is not set");
    process.exit(1);
  }

  const resolvers = [
    resolveMihomo,
    resolveMihomoAlpha,
    resolveClashRs,
    resolveClashPremium,
  ];
  consola.start("Resolving latest versions");
  const results = await Promise.all(resolvers.map((r) => r()));
  consola.success("Resolved latest versions");

  consola.start("Generating manifest");
  const manifest: ManifestVersion = {
    manifest_version: MANIFEST_VERSION,
    latest: {},
    arch_template: {},
  } as ManifestVersion;
  for (const result of results) {
    manifest.latest[result.name as SupportedCore] = result.version;
    manifest.arch_template[result.name as SupportedCore] = result.archMapping;
  }

  await fs.ensureDir(MANIFEST_DIR);
  // If no changes, skip writing manifest
  const previousManifest = (await fs.readJSON(MANIFEST_VERSION_PATH)) || {};
  delete previousManifest.updated_at;
  if (JSON.stringify(previousManifest) === JSON.stringify(manifest)) {
    consola.success("No changes, skip writing manifest");
    return;
  }
  manifest.updated_at = new Date().toISOString();
  consola.success("Generated manifest");

  await fs.writeJSON(MANIFEST_VERSION_PATH, manifest, { spaces: 2 });
  consola.success("Manifest written");
}

main();
