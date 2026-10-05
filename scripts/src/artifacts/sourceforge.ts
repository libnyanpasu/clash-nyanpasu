import { createHash } from "node:crypto";

export interface SourceforgeArtifact {
  fileName: string;
  fileSize: number;
  sha256: string;
  path: string;
}

export interface SourceforgeReleaseMirrorManifest {
  schemaVersion: 1;
  project: string;
  channel: "release";
  buildId: string;
  remotePath: string;
  assets: Record<string, { fileSize: number; sha256: string; url: string }>;
}

export interface SourceforgeReleaseCandidate {
  target: string;
  artifacts: Array<
    Pick<SourceforgeArtifact, "fileName" | "fileSize" | "sha256">
  >;
}

function validateReleaseMirrorManifest(
  value: unknown,
): SourceforgeReleaseMirrorManifest {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Existing SourceForge release mirror manifest is invalid");
  }
  const manifest = value as Record<string, unknown>;
  if (
    manifest.schemaVersion !== 1 || manifest.channel !== "release" ||
    typeof manifest.project !== "string" ||
    typeof manifest.buildId !== "string" ||
    typeof manifest.remotePath !== "string" ||
    typeof manifest.assets !== "object" || manifest.assets === null ||
    Array.isArray(manifest.assets)
  ) {
    throw new Error("Existing SourceForge release mirror manifest is invalid");
  }
  const assets: SourceforgeReleaseMirrorManifest["assets"] = {};
  for (const [fileName, raw] of Object.entries(manifest.assets)) {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
      throw new Error(
        `Existing SourceForge metadata is invalid for ${fileName}`,
      );
    }
    const asset = raw as Record<string, unknown>;
    if (
      !Number.isSafeInteger(asset.fileSize) || Number(asset.fileSize) <= 0 ||
      typeof asset.sha256 !== "string" ||
      !/^[a-f0-9]{64}$/.test(asset.sha256) ||
      typeof asset.url !== "string"
    ) {
      throw new Error(
        `Existing SourceForge metadata is invalid for ${fileName}`,
      );
    }
    assets[fileName] = {
      fileSize: Number(asset.fileSize),
      sha256: asset.sha256,
      url: asset.url,
    };
  }
  return {
    schemaVersion: 1,
    project: manifest.project,
    channel: "release",
    buildId: manifest.buildId,
    remotePath: manifest.remotePath,
    assets,
  };
}

export function assertReleaseMirrorMatchesPrior(
  project: string,
  releaseTag: string,
  candidate: SourceforgeReleaseCandidate,
  previousValue: unknown,
): void {
  const previous = validateReleaseMirrorManifest(previousValue);
  if (
    previous.project !== project || previous.buildId !== releaseTag ||
    previous.remotePath !== `releases/${releaseTag}`
  ) {
    throw new Error("Existing SourceForge metadata belongs to another release");
  }
  const candidateAssets = new Map(candidate.artifacts.map((artifact) => [
    artifact.fileName,
    artifact,
  ]));
  if (
    candidateAssets.size !== candidate.artifacts.length ||
    (candidate.target === "release-backfill" &&
      candidateAssets.size !== Object.keys(previous.assets).length) ||
    [...candidateAssets].some(([name, current]) => {
      const prior = previous.assets[name];
      return !prior || current.fileSize !== prior.fileSize ||
        current.sha256 !== prior.sha256 ||
        prior.url !== sourceforgeDownloadUrl(
            project,
            `releases/${releaseTag}`,
            name,
          );
    })
  ) {
    throw new Error(
      `SourceForge release inventory is immutable for ${releaseTag} (${candidate.target})`,
    );
  }
}

export async function guardReleaseMirrorUpload(
  project: string,
  releaseTag: string,
  candidate: SourceforgeReleaseCandidate,
  loadPrevious: () => Promise<unknown | null>,
  upload: () => Promise<void>,
  reserveArtifacts: () => Promise<void>,
): Promise<void> {
  const previous = await loadPrevious();
  if (previous !== null) {
    assertReleaseMirrorMatchesPrior(project, releaseTag, candidate, previous);
  }
  await reserveArtifacts();
  await upload();
}

/** An exclusive remote directory reserves the intended bytes before any upload. */
export async function reserveSourceforgeArtifact(
  artifact: Pick<SourceforgeArtifact, "fileName" | "fileSize" | "sha256">,
  createInventory: () => Promise<void>,
  loadInventory: () => Promise<unknown>,
): Promise<void> {
  try {
    await createInventory();
  } catch (creationError) {
    // A failed claim may already exist; absent or unreadable inventories fail closed.
    let value: unknown;
    try {
      value = await loadInventory();
    } catch (readError) {
      const message = (error: unknown) =>
        error instanceof Error ? error.message : String(error);
      throw new Error(
        `Could not reserve SourceForge artifact ${artifact.fileName}: creation failed: ${
          message(creationError)
        }; existing inventory read failed: ${message(readError)}`,
        { cause: new AggregateError([creationError, readError]) },
      );
    }
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      throw new Error(
        `Invalid SourceForge upload inventory for ${artifact.fileName}`,
      );
    }
    const previous = value as Record<string, unknown>;
    if (
      previous.fileName !== artifact.fileName ||
      previous.fileSize !== artifact.fileSize ||
      previous.sha256 !== artifact.sha256
    ) {
      throw new Error(
        `SourceForge file inventory is immutable for ${artifact.fileName}`,
      );
    }
  }
}

export function validateSourceforgeProject(project: string): void {
  if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(project)) {
    throw new Error(
      "SOURCEFORGE_PROJECT must be a valid lowercase project slug",
    );
  }
}

export function validateSourceforgeUsername(username: string): void {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(username)) {
    throw new Error("SOURCEFORGE_USERNAME must be a valid account username");
  }
}

export function validateSourceforgeFilename(fileName: string): void {
  if (
    !/^[A-Za-z0-9][A-Za-z0-9 _.,=+#@()-]*$/.test(fileName) ||
    fileName.endsWith(" ")
  ) {
    throw new Error(`Unsupported SourceForge filename: ${fileName}`);
  }
}

export function validateSourceforgeRemotePath(remotePath: string): void {
  const nightly = /^nightly\/[0-9]+-[0-9]+-[a-f0-9]{40}$/.test(remotePath);
  const release =
    /^releases\/(?!latest$|current$|stable$)[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/
      .test(remotePath);
  if (!nightly && !release) {
    throw new Error(`Invalid immutable SourceForge path: ${remotePath}`);
  }
}

export function sourceforgeDownloadUrl(
  project: string,
  remotePath: string,
  fileName: string,
): string {
  validateSourceforgeProject(project);
  validateSourceforgeRemotePath(remotePath);
  validateSourceforgeFilename(fileName);
  return `https://downloads.sourceforge.net/project/${project}/${remotePath}/${
    encodeURIComponent(fileName)
  }`;
}

export function sftpQuote(value: string): string {
  if (/[\r\n\0]/.test(value)) {
    throw new Error("SFTP path contains an unsupported control character");
  }
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

export function sourceforgeUploadBatch(
  project: string,
  remotePath: string,
  artifacts: Array<Pick<SourceforgeArtifact, "path" | "fileName">>,
  windows: boolean,
): string {
  validateSourceforgeProject(project);
  validateSourceforgeRemotePath(remotePath);
  const remoteDir = `/home/frs/project/${project}/${remotePath}`;
  const category = remotePath.split("/")[0];
  return [
    `-mkdir ${sftpQuote(`/home/frs/project/${project}/${category}`)}`,
    `-mkdir ${sftpQuote(remoteDir)}`,
    ...artifacts.map(({ path, fileName }) => {
      validateSourceforgeFilename(fileName);
      const localPath = windows ? path.replaceAll("\\", "/") : path;
      return `put ${sftpQuote(localPath)} ${
        sftpQuote(`${remoteDir}/${fileName}`)
      }`;
    }),
    "",
  ].join("\n");
}

export function sourceforgeReservationBatch(
  project: string,
  remotePath: string,
  fileName: string,
  localInventory: string,
): { batch: string; inventoryPath: string } {
  validateSourceforgeProject(project);
  validateSourceforgeRemotePath(remotePath);
  validateSourceforgeFilename(fileName);
  const remoteDir = `/home/frs/project/${project}/${remotePath}`;
  const inventoryRoot = `${remoteDir}/upload-inventory`;
  const inventoryDir = `${inventoryRoot}/${fileName}`;
  const batch = [
    `-mkdir ${
      sftpQuote(`/home/frs/project/${project}/${remotePath.split("/")[0]}`)
    }`,
    `-mkdir ${sftpQuote(remoteDir)}`,
    `-mkdir ${sftpQuote(inventoryRoot)}`,
    // The artifact directory is an exclusive claim, unlike shared parent directories.
    `mkdir ${sftpQuote(inventoryDir)}`,
    `put ${sftpQuote(localInventory)} ${
      sftpQuote(`${inventoryDir}/inventory.json`)
    }`,
    "",
  ].join("\n");
  return { batch, inventoryPath: `${inventoryDir}/inventory.json` };
}

export function sourceforgeWebUploadBatch(
  project: string,
  artifacts: Array<{ path: string; fileName: string; stagedName: string }>,
): string {
  validateSourceforgeProject(project);
  const directory = `/home/project-web/${project}/htdocs/updater`;
  return [
    `-mkdir ${sftpQuote(`/home/project-web/${project}/htdocs`)}`,
    `-mkdir ${sftpQuote(directory)}`,
    ...artifacts.map(({ path, fileName, stagedName }) => {
      validateSourceforgeFilename(fileName);
      validateSourceforgeFilename(stagedName);
      const localPath = Deno.build.os === "windows"
        ? path.replaceAll("\\", "/")
        : path;
      const stagedPath = `${directory}/${stagedName}`;
      return `put ${sftpQuote(localPath)} ${sftpQuote(stagedPath)}`;
    }),
    ...artifacts.map(({ fileName, stagedName }) =>
      `rename ${sftpQuote(`${directory}/${stagedName}`)} ${
        sftpQuote(`${directory}/${fileName}`)
      }`
    ),
    "",
  ].join("\n");
}

export async function hashFile(
  filePath: string,
): Promise<{ fileSize: number; sha256: string; md5: string }> {
  const sha256 = createHash("sha256");
  const md5 = createHash("md5");
  const file = await Deno.open(filePath, { read: true });
  let fileSize = 0;
  try {
    const chunk = new Uint8Array(1024 * 1024);
    while (true) {
      const size = await file.read(chunk);
      if (size === null) break;
      fileSize += size;
      sha256.update(chunk.subarray(0, size));
      md5.update(chunk.subarray(0, size));
    }
  } finally {
    file.close();
  }
  return { fileSize, sha256: sha256.digest("hex"), md5: md5.digest("hex") };
}

export function validateSourceforgeReportTargets(
  reports: Array<{ target?: string; status?: string }>,
  expectedTargets: readonly string[],
): string[] {
  const issues: string[] = [];
  const seen = new Set<string>();
  for (const report of reports) {
    if (!report.target || seen.has(report.target)) {
      issues.push(`missing or duplicate target: ${report.target ?? "(unset)"}`);
    } else {
      seen.add(report.target);
    }
    if (report.status !== "uploaded") {
      issues.push(
        `${report.target ?? "unknown"}: status is ${
          report.status ?? "missing"
        }`,
      );
    }
  }
  for (const target of expectedTargets) {
    if (!seen.has(target)) issues.push(`${target}: upload report is missing`);
  }
  return issues;
}
