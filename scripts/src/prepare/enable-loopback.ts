import * as path from "jsr:@std/path";

export interface EnableLoopbackAsset {
  assetName: string;
  downloadURL: string;
  checksumURL: string;
}

export interface EnableLoopbackInfo extends EnableLoopbackAsset {
  version: string;
  /** Windows builds before Windows 11 24H1 load it beside the executable. */
  bootstrapper: EnableLoopbackAsset;
}

export function parseEnableLoopbackVersion(manifest: string): string {
  const version = manifest.match(
    /^\[package\]\s*\r?\n([^[]*)/m,
  )?.[1].match(/^version\s*=\s*["']([^"']+)["']\s*(?:#.*)?$/m)?.[1];
  if (!version || !/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(version)) {
    throw new Error("failed to parse enable-loopback package version");
  }
  return version;
}

export function enableLoopbackInfo(
  version: string,
  arch: string,
): EnableLoopbackInfo {
  const target = arch === "x64"
    ? "x86_64-pc-windows-msvc"
    : arch === "arm64"
    ? "aarch64-pc-windows-msvc"
    : undefined;
  if (!target) {
    throw new Error(`unsupported enable-loopback architecture: ${arch}`);
  }
  return {
    version,
    ...releaseAsset(version, `enableLoopback-${target}.exe`),
    bootstrapper: releaseAsset(
      version,
      `Microsoft.WindowsAppRuntime.Bootstrap-${target}.dll`,
    ),
  };
}

function releaseAsset(version: string, assetName: string): EnableLoopbackAsset {
  const downloadURL =
    `https://github.com/libnyanpasu/enable-loopback/releases/download/v${version}/${assetName}`;
  return { assetName, downloadURL, checksumURL: `${downloadURL}.sha256` };
}

export async function getEnableLoopbackInfo(
  workspaceRoot: string,
  arch: string,
): Promise<EnableLoopbackInfo> {
  const manifest = await Deno.readTextFile(path.join(
    workspaceRoot,
    "backend/enable-loopback/crates/enable-loopback/Cargo.toml",
  ));
  return enableLoopbackInfo(parseEnableLoopbackVersion(manifest), arch);
}

export function parseEnableLoopbackChecksum(
  checksum: string,
  assetName: string,
): string {
  const match = checksum.trim().match(/^([a-fA-F0-9]{64}) {2}([^\r\n]+)$/);
  if (!match || match[2] !== assetName) {
    throw new Error(`invalid SHA-256 checksum for ${assetName}`);
  }
  return match[1].toLowerCase();
}

export async function verifyEnableLoopbackChecksum(
  bytes: Uint8Array<ArrayBuffer>,
  checksum: string,
  assetName: string,
): Promise<void> {
  const expected = parseEnableLoopbackChecksum(checksum, assetName);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const actual = Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0")).join("");
  if (actual !== expected) {
    throw new Error(
      `enable-loopback SHA-256 mismatch: expected ${expected}, got ${actual}`,
    );
  }
}
