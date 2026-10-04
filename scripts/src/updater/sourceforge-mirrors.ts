export interface SourceforgeMirrorAsset {
  url: string;
  fileSize: number;
  sha256: string;
}

export interface SourceforgeMirrorManifest {
  schemaVersion: 1;
  project: string;
  channel: "release" | "nightly";
  buildId: string;
  remotePath: string;
  publishedAt?: string;
  assets: Record<string, SourceforgeMirrorAsset>;
}

export interface UpdaterPlatform {
  signature: string;
  url: string;
  mirrors?: { sourceforge: string };
  project?: string;
  build_id?: string;
}

export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
  size?: number;
}

const projectPattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
const assetNamePattern = /^[A-Za-z0-9][A-Za-z0-9 _.,=+#@()-]*$/;
export const SOURCEFORGE_MIRRORS_ASSET_NAME = "sourceforge-mirrors.json";

function isCanonicalUtcIso(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value)
  ) {
    return false;
  }
  const date = new Date(value);
  return !Number.isNaN(date.getTime()) && date.toISOString() === value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validateAssetUrl(
  urlValue: unknown,
  project: string,
  remotePath: string,
  fileName: string,
): urlValue is string {
  if (typeof urlValue !== "string") return false;
  try {
    const url = new URL(urlValue);
    const segments = url.pathname.split("/").map(decodeURIComponent);
    return url.protocol === "https:" &&
      url.hostname === "downloads.sourceforge.net" && !url.port &&
      !url.username && !url.password && !url.search && !url.hash &&
      segments.length === 6 &&
      segments.join("/") === `/project/${project}/${remotePath}/${fileName}`;
  } catch {
    return false;
  }
}

/** Parse and validate the byte-verified SourceForge output from the CI gate. */
export function parseSourceforgeMirrorManifest(
  value: string | unknown,
): SourceforgeMirrorManifest {
  const parsed = typeof value === "string"
    ? JSON.parse(value) as unknown
    : value;
  if (!isRecord(parsed)) {
    throw new Error("SourceForge mirror manifest must be an object");
  }
  if (parsed.schemaVersion !== 1) {
    throw new Error("Unsupported SourceForge mirror schema");
  }
  if (
    typeof parsed.project !== "string" || !projectPattern.test(parsed.project)
  ) {
    throw new Error("Invalid SourceForge project in mirror manifest");
  }
  if (parsed.channel !== "release" && parsed.channel !== "nightly") {
    throw new Error("Invalid SourceForge channel in mirror manifest");
  }
  if (typeof parsed.buildId !== "string") {
    throw new Error("Missing SourceForge build identity");
  }
  if (
    parsed.channel === "release" &&
    !/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(parsed.buildId)
  ) {
    throw new Error("Invalid SourceForge release tag");
  }
  if (
    parsed.channel === "nightly" &&
    !/^\d+-\d+-[a-f0-9]{40}$/.test(parsed.buildId)
  ) {
    throw new Error(
      "Nightly SourceForge build id must include the full commit SHA",
    );
  }
  const expectedRemotePath = `${
    parsed.channel === "nightly" ? "nightly" : "releases"
  }/${parsed.buildId}`;
  if (parsed.remotePath !== expectedRemotePath) {
    throw new Error(
      "SourceForge mirror path does not match its immutable build identity",
    );
  }
  if (
    parsed.publishedAt !== undefined &&
    !isCanonicalUtcIso(parsed.publishedAt)
  ) {
    throw new Error("Invalid SourceForge publication timestamp");
  }
  if (!isRecord(parsed.assets) || Object.keys(parsed.assets).length === 0) {
    throw new Error("SourceForge mirror manifest has no assets");
  }

  const assets: Record<string, SourceforgeMirrorAsset> = {};
  for (const [fileName, rawAsset] of Object.entries(parsed.assets)) {
    if (!assetNamePattern.test(fileName) || fileName.endsWith(" ")) {
      throw new Error(`Invalid SourceForge asset name: ${fileName}`);
    }
    if (
      !isRecord(rawAsset) ||
      !Number.isSafeInteger(rawAsset.fileSize) ||
      Number(rawAsset.fileSize) <= 0 ||
      typeof rawAsset.sha256 !== "string" ||
      !/^[a-f0-9]{64}$/.test(rawAsset.sha256) ||
      !validateAssetUrl(
        rawAsset.url,
        parsed.project,
        parsed.remotePath,
        fileName,
      )
    ) {
      throw new Error(`Invalid SourceForge asset metadata for ${fileName}`);
    }
    assets[fileName] = {
      url: rawAsset.url as string,
      fileSize: rawAsset.fileSize as number,
      sha256: rawAsset.sha256,
    };
  }

  return {
    schemaVersion: 1,
    project: parsed.project,
    channel: parsed.channel,
    buildId: parsed.buildId,
    remotePath: parsed.remotePath,
    ...(parsed.publishedAt === undefined
      ? {}
      : { publishedAt: parsed.publishedAt }),
    assets,
  };
}

/** Stable serialization lets retries detect identical per-release metadata. */
export function canonicalSourceforgeMirrorManifest(
  manifest: SourceforgeMirrorManifest,
): string {
  return JSON.stringify({
    schemaVersion: manifest.schemaVersion,
    project: manifest.project,
    channel: manifest.channel,
    buildId: manifest.buildId,
    remotePath: manifest.remotePath,
    ...(manifest.publishedAt === undefined
      ? {}
      : { publishedAt: manifest.publishedAt }),
    assets: Object.fromEntries(
      Object.entries(manifest.assets).sort(([left], [right]) =>
        left.localeCompare(right)
      ),
    ),
  });
}

/** Compare immutable build identity and asset bytes while ignoring batch time. */
export function sourceforgeMirrorAssetsEqual(
  left: SourceforgeMirrorManifest,
  right: SourceforgeMirrorManifest,
): boolean {
  const immutableContent = (manifest: SourceforgeMirrorManifest) =>
    JSON.stringify({
      schemaVersion: manifest.schemaVersion,
      project: manifest.project,
      channel: manifest.channel,
      buildId: manifest.buildId,
      remotePath: manifest.remotePath,
      assets: Object.fromEntries(
        Object.entries(manifest.assets).sort(([a], [b]) => a.localeCompare(b)),
      ),
    });
  return immutableContent(left) === immutableContent(right);
}

function basenameFromUrl(value: string): string {
  const url = new URL(value);
  const segment = url.pathname.split("/").at(-1);
  if (!segment) throw new Error("Updater asset URL has no filename");
  return decodeURIComponent(segment);
}

/** Attach only mirrors whose binaries and signature files belong to this exact release. */
function attachMirrorsToPlatforms(
  platforms: Record<string, UpdaterPlatform>,
  releaseAssets: readonly ReleaseAsset[],
  manifest: SourceforgeMirrorManifest,
): Record<string, UpdaterPlatform> {
  const githubAssets = new Map(
    releaseAssets.map((asset) => [asset.name, asset]),
  );
  const result: Record<string, UpdaterPlatform> = {};
  for (const [target, platform] of Object.entries(platforms)) {
    const fileName = basenameFromUrl(platform.url);
    const signatureName = `${fileName}.sig`;
    const mirror = manifest.assets[fileName];
    const signatureMirror = manifest.assets[signatureName];
    const githubAsset = githubAssets.get(fileName);
    const githubSignature = githubAssets.get(signatureName);
    if (!mirror || !signatureMirror || !githubAsset || !githubSignature) {
      throw new Error(
        `SourceForge mirror is missing ${fileName} or its signed metadata`,
      );
    }
    if (
      (githubAsset.size !== undefined &&
        githubAsset.size !== mirror.fileSize) ||
      (githubSignature.size !== undefined &&
        githubSignature.size !== signatureMirror.fileSize)
    ) {
      throw new Error(
        `SourceForge mirror size differs from release assets for ${fileName}`,
      );
    }
    result[target] = {
      ...platform,
      mirrors: { sourceforge: mirror.url },
      project: manifest.project,
      build_id: manifest.buildId,
    };
  }
  return result;
}

export function attachSourceforgeMirrors(
  platforms: Record<string, UpdaterPlatform>,
  releaseAssets: readonly ReleaseAsset[],
  manifest: SourceforgeMirrorManifest | undefined,
  selectedReleaseTag: string,
): Record<string, UpdaterPlatform> {
  if (!manifest) return platforms;
  if (
    manifest.channel !== "release" || manifest.buildId !== selectedReleaseTag
  ) {
    throw new Error(
      `SourceForge release identity ${manifest.buildId} does not match selected release ${selectedReleaseTag}`,
    );
  }
  return attachMirrorsToPlatforms(platforms, releaseAssets, manifest);
}

/** A nightly mirror is accepted only for this run's full source SHA and announced hash. */
export function validateNightlySourceforgeIdentity(
  manifest: SourceforgeMirrorManifest,
  fullCommitSha: string,
  announcedHash: string,
  runId: string,
  runAttempt: string,
): void {
  if (!/^[a-f0-9]{40}$/.test(fullCommitSha)) {
    throw new Error(
      "GITHUB_SHA must be a full 40-character commit SHA for nightly mirrors",
    );
  }
  if (manifest.channel !== "nightly") {
    throw new Error("Expected nightly SourceForge metadata");
  }
  const match = /^(\d+)-(\d+)-([a-f0-9]{40})$/.exec(manifest.buildId);
  if (
    !match || match[1] !== runId || match[2] !== runAttempt ||
    match[3] !== fullCommitSha
  ) {
    throw new Error(
      "Nightly SourceForge mirror does not match this run, attempt, and full commit",
    );
  }
  if (!announcedHash || !fullCommitSha.startsWith(announcedHash)) {
    throw new Error(
      "Nightly announced version does not match the mirrored commit",
    );
  }
}

export function attachNightlySourceforgeMirrors(
  platforms: Record<string, UpdaterPlatform>,
  releaseAssets: readonly ReleaseAsset[],
  manifest: SourceforgeMirrorManifest | undefined,
  fullCommitSha: string,
  announcedHash: string,
  runId: string,
  runAttempt: string,
): Record<string, UpdaterPlatform> {
  if (!manifest) return platforms;
  validateNightlySourceforgeIdentity(
    manifest,
    fullCommitSha,
    announcedHash,
    runId,
    runAttempt,
  );
  return attachMirrorsToPlatforms(platforms, releaseAssets, manifest);
}

/** Rewrite only the primary updater URL; a SourceForge mirror stays direct. */
export function mapUpdaterPlatformUrls(
  platforms: Record<string, UpdaterPlatform>,
  mapUrl: (url: string) => string,
): Record<string, UpdaterPlatform> {
  return Object.fromEntries(
    Object.entries(platforms).map(([target, platform]) => [
      target,
      { ...platform, url: mapUrl(platform.url) },
    ]),
  );
}
