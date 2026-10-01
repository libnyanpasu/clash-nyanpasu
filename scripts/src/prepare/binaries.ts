import * as path from "jsr:@std/path";
import type { BinInfo, ClashManifest, VersionManifest } from "./types.ts";
import { mapArch } from "./platform.ts";
import {
  extractVersionFromAssetList,
  fetchText,
  isValidVersion,
  normalizeVersion,
} from "./versions.ts";
import type { DebugLog } from "./download.ts";

const MIHOMO_ALPHA_ASSETS_URL =
  "https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha";

export interface BinaryResolverOptions {
  versionManifest: VersionManifest;
  sidecarHost: string;
  platform: string;
  arch: string;
  workspaceRoot: string;
  debugLog?: DebugLog;
}

export function createBinaryResolvers(options: BinaryResolverOptions) {
  const { versionManifest, sidecarHost, platform, arch, workspaceRoot } =
    options;
  const debugLog = options.debugLog ?? (() => {});

  const clashManifest: ClashManifest = {
    URL_PREFIX: "https://github.com/Dreamacro/clash/releases/download/premium/",
    BACKUP_URL_PREFIX:
      "https://github.com/zhongfly/Clash-premium-backup/releases/download/",
    BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
    VERSION: versionManifest.latest.clash_premium,
    ARCH_MAPPING: versionManifest.arch_template.clash_premium,
  };

  const clashMetaManifest: ClashManifest = {
    URL_PREFIX:
      `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
    VERSION: versionManifest.latest.mihomo,
    ARCH_MAPPING: versionManifest.arch_template.mihomo,
  };

  const clashMetaAlphaManifest: ClashManifest = {
    VERSION_URL:
      "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
    URL_PREFIX:
      "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha",
    ARCH_MAPPING: versionManifest.arch_template.mihomo_alpha,
  };

  const clashRsManifest: ClashManifest = {
    URL_PREFIX: "https://github.com/ibigbug/clash-rs/releases/download/",
    VERSION: versionManifest.latest.clash_rs,
    ARCH_MAPPING: versionManifest.arch_template.clash_rs,
  };

  const clashRsAlphaManifest: ClashManifest = {
    URL_PREFIX: "https://github.com/ibigbug/clash-rs/releases/download/latest",
    ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha,
  };

  const meowManifest: ClashManifest = {
    URL_PREFIX: "https://github.com/madeye/meow-rs/releases/download/",
    VERSION: versionManifest.latest.meow,
    ARCH_MAPPING: versionManifest.arch_template.meow,
  };

  function targetName(name: string): string {
    return `${name}-${sidecarHost}${platform === "win32" ? ".exe" : ""}`;
  }

  function withMappedAsset(manifest: ClashManifest): {
    version?: string;
    assetName: string;
  } {
    const version = manifest.VERSION ?? manifest.BACKUP_LATEST_DATE;
    const assetName = manifest.ARCH_MAPPING[mapArch(platform, arch)]
      .replace("{}", version!);
    return { version, assetName };
  }

  function getClashBackupInfo(): BinInfo {
    const { BACKUP_LATEST_DATE, BACKUP_URL_PREFIX } = clashManifest;
    const { assetName } = withMappedAsset(clashManifest);
    const isWin = platform === "win32";
    const name = assetName;
    return {
      name: "clash",
      version: BACKUP_LATEST_DATE,
      targetFile: targetName("clash"),
      exeFile: `${name}${isWin ? ".exe" : ""}`,
      tmpFile: name,
      downloadURL: `${BACKUP_URL_PREFIX}${BACKUP_LATEST_DATE}/${name}`,
    };
  }

  function getClashMetaInfo(): BinInfo {
    const { URL_PREFIX } = clashMetaManifest;
    const { version, assetName } = withMappedAsset(clashMetaManifest);
    const isWin = platform === "win32";
    return {
      name: "mihomo",
      version,
      targetFile: targetName("mihomo"),
      exeFile: `${assetName}${isWin ? ".exe" : ""}`,
      tmpFile: assetName,
      downloadURL: `${URL_PREFIX}/${assetName}`,
    };
  }

  async function getClashMetaAlphaInfo(): Promise<BinInfo> {
    const { ARCH_MAPPING, URL_PREFIX, VERSION_URL } = clashMetaAlphaManifest;
    const assetTemplate = ARCH_MAPPING[mapArch(platform, arch)];
    const versionFromFile = normalizeVersion(
      (await fetchText(VERSION_URL!))?.trim(),
    );
    const versionFromAssets = extractVersionFromAssetList(
      (await fetchText(MIHOMO_ALPHA_ASSETS_URL)) ?? "",
      assetTemplate,
    );
    const fallbackVersion = normalizeVersion(
      versionManifest.latest.mihomo_alpha,
    );
    const version = [versionFromFile, versionFromAssets, fallbackVersion].find(
      isValidVersion,
    );
    if (!version) throw new Error("cannot resolve mihomo-alpha version");

    debugLog(`mihomo-alpha version: ${version}`);
    const name = assetTemplate.replace("{}", version);
    const isWin = platform === "win32";
    return {
      name: "mihomo-alpha",
      version,
      targetFile: targetName("mihomo-alpha"),
      exeFile: `${name}${isWin ? ".exe" : ""}`,
      tmpFile: name,
      downloadURL: `${URL_PREFIX}/${name}`,
    };
  }

  function getClashRustInfo(): BinInfo {
    const { URL_PREFIX, VERSION } = clashRsManifest;
    const { assetName } = withMappedAsset(clashRsManifest);
    return {
      name: "clash-rs",
      version: VERSION,
      targetFile: targetName("clash-rs"),
      exeFile: assetName,
      tmpFile: assetName,
      downloadURL: `${URL_PREFIX}${VERSION}/${assetName}`,
    };
  }

  function getClashRustAlphaInfo(): BinInfo {
    const { ARCH_MAPPING, URL_PREFIX } = clashRsAlphaManifest;
    const version = versionManifest.latest.clash_rs_alpha;
    debugLog(`clash-rs-alpha version: ${version}`);
    const name = ARCH_MAPPING[mapArch(platform, arch)].replace("{}", version);
    return {
      name: "clash-rs-alpha",
      version,
      targetFile: targetName("clash-rs-alpha"),
      exeFile: name,
      tmpFile: name,
      downloadURL: `${URL_PREFIX}/${name}`,
    };
  }

  function getMeowInfo(): BinInfo {
    const { URL_PREFIX, VERSION } = meowManifest;
    const { assetName } = withMappedAsset(meowManifest);
    return {
      name: "meow",
      version: VERSION,
      targetFile: targetName("meow"),
      exeFile: assetName,
      tmpFile: assetName,
      downloadURL: `${URL_PREFIX}${VERSION}/${assetName}`,
    };
  }

  async function getNyanpasuServiceInfo(): Promise<BinInfo> {
    const manifestPath = path.join(
      workspaceRoot,
      "backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml",
    );
    const manifest = await Deno.readTextFile(manifestPath);
    const match = manifest.match(
      /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms,
    );
    if (!match) {
      throw new Error(
        `failed to parse nyanpasu-service version from ${manifestPath}`,
      );
    }
    const version = `v${match[1]}`;
    debugLog(`nyanpasu-service version: ${version}`);
    const serviceRepo = "libnyanpasu/nyanpasu-runtime";
    const isWin = sidecarHost.includes("windows");
    const urlExt = isWin ? "zip" : "tar.gz";
    const name = "nyanpasu-service";
    return {
      name,
      version,
      targetFile: `${name}-${sidecarHost}${isWin ? ".exe" : ""}`,
      exeFile: `${name}${isWin ? ".exe" : ""}`,
      tmpFile: `${name}-${version}-${sidecarHost}.${urlExt}`,
      downloadURL:
        `https://github.com/${serviceRepo}/releases/download/${version}/${name}-${sidecarHost}.${urlExt}`,
    };
  }

  return {
    clash: getClashBackupInfo,
    mihomo: getClashMetaInfo,
    mihomoAlpha: getClashMetaAlphaInfo,
    clashRs: getClashRustInfo,
    clashRsAlpha: getClashRustAlphaInfo,
    meow: getMeowInfo,
    nyanpasuService: getNyanpasuServiceInfo,
  };
}
