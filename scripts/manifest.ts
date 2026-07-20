import { consola } from "./utils/logger.ts";

// === Types ===

export type SupportedArch =
  | "windows-x86_64"
  | "windows-arm64"
  | "linux-aarch64"
  | "linux-amd64"
  | "darwin-arm64"
  | "darwin-x64";

export type ArchMapping = Record<SupportedArch, string>;

export type LatestVersionResolver = Promise<{
  name: string;
  version: string;
  archMapping: ArchMapping;
}>;

// === GitHub API helpers ===

const GITHUB_API_HEADERS = {
  Accept: "application/vnd.github+json",
  "User-Agent": "clash-nyanpasu",
};

const MIHOMO_ALPHA_VERSION_URL =
  "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt";
const MIHOMO_ALPHA_ASSETS_URL =
  "https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha";

async function githubFetch<T>(url: string): Promise<T> {
  const resp = await fetch(url, { headers: GITHUB_API_HEADERS });
  if (!resp.ok) {
    throw new Error(
      `GitHub API error: ${resp.statusText} (${resp.status}) — ${url}`,
    );
  }
  return resp.json() as Promise<T>;
}

async function getLatestRelease(owner: string, repo: string): Promise<string> {
  const data = await githubFetch<{ tag_name: string }>(
    `https://api.github.com/repos/${owner}/${repo}/releases/latest`,
  );
  return data.tag_name;
}

function isValidVersion(version?: string): version is string {
  return Boolean(version && /^[A-Za-z0-9._+-]+$/.test(version));
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function extractVersionFromAssetList(
  page: string,
  assetTemplate: string,
): string | undefined {
  const [prefix, suffix] = assetTemplate.split("{}");
  if (prefix === undefined || suffix === undefined) return undefined;

  const matcher = new RegExp(
    `${escapeRegExp(prefix)}([A-Za-z0-9._+-]+)${escapeRegExp(suffix)}`,
  );
  const matched = page.match(matcher)?.[1];
  return isValidVersion(matched) ? matched : undefined;
}

async function fetchText(url: string): Promise<string | undefined> {
  const resp = await fetch(url, { headers: GITHUB_API_HEADERS });
  if (!resp.ok) {
    return undefined;
  }
  return await resp.text();
}

// === Resolvers ===

export const resolveMihomo = async (): LatestVersionResolver => {
  const version = await getLatestRelease("MetaCubeX", "mihomo");
  consola.debug(`mihomo latest release: ${version}`);

  const archMapping: ArchMapping = {
    "windows-x86_64": "mihomo-windows-amd64-v2-{}.zip",
    "windows-arm64": "mihomo-windows-arm64-{}.zip",
    "linux-aarch64": "mihomo-linux-arm64-{}.gz",
    "linux-amd64": "mihomo-linux-amd64-v2-{}.gz",
    "darwin-arm64": "mihomo-darwin-arm64-{}.gz",
    "darwin-x64": "mihomo-darwin-amd64-v2-{}.gz",
  };

  return { name: "mihomo", version, archMapping };
};

export const resolveMihomoAlpha = async (): LatestVersionResolver => {
  const archMapping: ArchMapping = {
    "windows-x86_64": "mihomo-windows-amd64-v2-{}.zip",
    "windows-arm64": "mihomo-windows-arm64-{}.zip",
    "linux-aarch64": "mihomo-linux-arm64-{}.gz",
    "linux-amd64": "mihomo-linux-amd64-v2-{}.gz",
    "darwin-arm64": "mihomo-darwin-arm64-{}.gz",
    "darwin-x64": "mihomo-darwin-amd64-v2-{}.gz",
  };

  const versionFromFile = (await fetchText(MIHOMO_ALPHA_VERSION_URL))?.trim();
  const versionFromAssets = extractVersionFromAssetList(
    (await fetchText(MIHOMO_ALPHA_ASSETS_URL)) ?? "",
    archMapping["windows-x86_64"],
  );
  const alphaReleaseHash = [versionFromFile, versionFromAssets].find(
    isValidVersion,
  );

  if (!alphaReleaseHash) {
    throw new Error("Unable to resolve mihomo alpha version");
  }

  consola.debug(`mihomo alpha release: ${alphaReleaseHash}`);

  return { name: "mihomo_alpha", version: alphaReleaseHash, archMapping };
};

export const resolveClashRs = async (): LatestVersionResolver => {
  const version = await getLatestRelease("Watfaq", "clash-rs");
  consola.debug(`clash-rs latest release: ${version}`);

  const archMapping: ArchMapping = {
    "windows-x86_64": "clash-rs-x86_64-pc-windows-msvc.exe",
    "windows-arm64": "clash-rs-aarch64-pc-windows-msvc.exe",
    "linux-aarch64": "clash-rs-aarch64-unknown-linux-gnu",
    "linux-amd64": "clash-rs-x86_64-unknown-linux-gnu-static-crt",
    "darwin-arm64": "clash-rs-aarch64-apple-darwin",
    "darwin-x64": "clash-rs-x86_64-apple-darwin",
  };

  return { name: "clash_rs", version, archMapping };
};

export const resolveClashRsAlpha = async (): LatestVersionResolver => {
  // Fetch commit SHA for the "latest" pre-release tag and the stable base version in parallel
  const [ref, stableTag] = await Promise.all([
    githubFetch<{ object: { type: string; sha: string; url: string } }>(
      "https://api.github.com/repos/Watfaq/clash-rs/git/ref/tags/latest",
    ),
    getLatestRelease("Watfaq", "clash-rs"),
  ]);

  // Dereference annotated tags to get the underlying commit SHA
  let commitSha = ref.object.sha;
  if (ref.object.type === "tag") {
    const tagObj = await githubFetch<{ object: { sha: string } }>(
      ref.object.url,
    );
    commitSha = tagObj.object.sha;
  }

  const shortSha = commitSha.substring(0, 7);
  const baseVersion = stableTag.replace(/^v/, "");
  const alphaVersion = `${baseVersion}-alpha+sha.${shortSha}`;
  consola.debug(`clash-rs alpha latest release: ${alphaVersion}`);

  const archMapping: ArchMapping = {
    "windows-x86_64": "clash-rs-x86_64-pc-windows-msvc.exe",
    "windows-arm64": "clash-rs-aarch64-pc-windows-msvc.exe",
    "linux-aarch64": "clash-rs-aarch64-unknown-linux-gnu",
    "linux-amd64": "clash-rs-x86_64-unknown-linux-gnu-static-crt",
    "darwin-arm64": "clash-rs-aarch64-apple-darwin",
    "darwin-x64": "clash-rs-x86_64-apple-darwin",
  };

  return { name: "clash_rs_alpha", version: alphaVersion, archMapping };
};

export const resolveClashPremium = async (): LatestVersionResolver => {
  const version = await getLatestRelease("zhongfly", "Clash-premium-backup");
  consola.debug(`clash-premium latest release: ${version}`);

  const archMapping: ArchMapping = {
    "windows-x86_64": "clash-windows-amd64-n{}.zip",
    "windows-arm64": "clash-windows-arm64-n{}.zip",
    "linux-aarch64": "clash-linux-arm64-n{}.gz",
    "linux-amd64": "clash-linux-amd64-n{}.gz",
    "darwin-arm64": "clash-darwin-arm64-n{}.gz",
    "darwin-x64": "clash-darwin-amd64-n{}.gz",
  };

  return { name: "clash_premium", version, archMapping };
};

export const resolveMeow = async (): LatestVersionResolver => {
  const version = await getLatestRelease("madeye", "meow-rs");
  consola.debug(`meow-rs latest release: ${version}`);

  const archMapping: ArchMapping = {
    "windows-x86_64": "meow-{}-x86_64-pc-windows-msvc.zip",
    "windows-arm64": "meow-{}-aarch64-pc-windows-msvc.zip",
    "linux-aarch64": "meow-{}-aarch64-unknown-linux-musl.tar.gz",
    "linux-amd64": "meow-{}-x86_64-unknown-linux-musl.tar.gz",
    "darwin-arm64": "meow-{}-aarch64-apple-darwin.tar.gz",
    "darwin-x64": "meow-{}-x86_64-apple-darwin.tar.gz",
  };

  return { name: "meow", version, archMapping };
};
