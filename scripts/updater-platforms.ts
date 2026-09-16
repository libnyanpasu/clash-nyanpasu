export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

export interface UpdaterPlatform {
  signature: string;
  url: string;
}

export const UPDATER_TARGETS = [
  "win64",
  "linux",
  "darwin",
  "darwin-aarch64",
  "darwin-intel",
  "darwin-x86_64",
  "linux-x86_64",
  "windows-x86_64",
  "windows-aarch64",
  "windows-x86_64-fixed-webview",
  "windows-aarch64-fixed-webview",
] as const;

function targetsForArchive(name: string): string[] {
  if (name.endsWith(".nsis.zip")) {
    const fixed = name.includes("fixed-webview");
    if (name.includes("x64")) {
      return fixed
        ? ["windows-x86_64-fixed-webview"]
        : ["win64", "windows-x86_64"];
    }
    if (name.includes("arm64")) {
      return [fixed ? "windows-aarch64-fixed-webview" : "windows-aarch64"];
    }
  }
  if (name.endsWith("aarch64.app.tar.gz")) return ["darwin-aarch64"];
  if (name.endsWith(".app.tar.gz") && !name.includes("aarch")) {
    return ["darwin", "darwin-intel", "darwin-x86_64"];
  }
  if (name.endsWith(".AppImage.tar.gz")) return ["linux", "linux-x86_64"];
  return [];
}

export async function collectUpdaterPlatforms(
  assets: readonly ReleaseAsset[],
  loadSignature: (url: string) => Promise<string>,
): Promise<Record<string, UpdaterPlatform>> {
  const byName = new Map(assets.map((asset) => [asset.name, asset]));
  const platforms: Record<string, UpdaterPlatform> = {};
  for (const asset of assets) {
    const targets = targetsForArchive(asset.name);
    if (targets.length === 0) continue;
    const signatureAsset = byName.get(`${asset.name}.sig`);
    if (!signatureAsset) throw new Error(`Missing signature for ${asset.name}`);
    const signature = (await loadSignature(signatureAsset.browser_download_url))
      .trim();
    if (!signature) throw new Error(`Empty signature for ${asset.name}`);
    for (const target of targets) {
      if (platforms[target]) {
        throw new Error(`Multiple updater archives for ${target}`);
      }
      platforms[target] = { signature, url: asset.browser_download_url };
    }
  }
  return platforms;
}
