import { createHash } from "node:crypto";
import { basename, dirname, resolve } from "node:path";

interface PublishArtifact {
  path: string;
  fileName: string;
  fileSize: number;
  sha256: string;
  md5: string;
}

interface PublishManifest {
  schemaVersion: 1;
  buildId: string;
  itemIdentifier: string;
  channel: "release" | "nightly";
  commit: string;
  publishedAt?: string;
  tag: string | null;
  target?: string | null;
  folderPath: string;
  artifacts: PublishArtifact[];
}

interface RegisteredArtifact {
  fileId: string;
  fileName: string;
  fileSize: number;
  storageKey: string;
  downloadUrl: string;
  status: "pending" | "ready" | "failed";
}

interface BuildResponse {
  buildId: string;
  itemIdentifier: string;
  schemaVersion?: 1;
  target?: string | null;
  publishedAt?: string;
  channel?: "release" | "nightly";
  commit?: string;
  folderPath?: string;
  status: "pending" | "ready" | "failed";
  diagnostics: string[];
  artifacts: RegisteredArtifact[];
}

const RETRY_COUNT = 5;
const IA_S3_ORIGIN = "https://s3.us.archive.org";
const IA_METADATA_ORIGIN = "https://archive.org";
const IA_SOURCE = "https://github.com/libnyanpasu/clash-nyanpasu";
const IA_HOMEPAGE = "https://nyanpasu.org";
const IA_LICENSE_URL = "https://www.gnu.org/licenses/gpl-3.0.html";

const parseOptions = (args: string[]) => {
  const options = new Map<string, string>();
  for (let i = 0; i < args.length; i++) {
    const key = args[i];
    if (key === "--verify-only" || key === "--register-only") {
      options.set(key, "true");
      continue;
    }
    if (!key.startsWith("--") || !args[i + 1] || args[i + 1].startsWith("--")) {
      throw new Error(`Expected --option value, got ${key}`);
    }
    options.set(key, args[++i]);
  }
  const manifest = options.get("--manifest");
  const server = options.get("--server");
  const verifyOnly = options.get("--verify-only") === "true";
  const registerOnly = options.get("--register-only") === "true";
  if (verifyOnly && registerOnly) {
    throw new Error("--verify-only and --register-only are mutually exclusive");
  }
  const buildId = options.get("--build-id");
  if (!server || (verifyOnly ? !buildId : !manifest)) {
    throw new Error(
      "usage: archive:publish --manifest <json> --server <https-url> [--report <json>] | archive:verify --build-id <id> --server <https-url> [--report <json>]",
    );
  }
  const serverUrl = new URL(server);
  if (serverUrl.protocol !== "https:") {
    throw new Error("--server must use HTTPS");
  }
  if (
    serverUrl.hostname !== "archive.nyanpasu.org" &&
    serverUrl.hostname !== "localhost" &&
    serverUrl.hostname !== "127.0.0.1"
  ) {
    throw new Error(
      "--server must be archive.nyanpasu.org or a local test server",
    );
  }
  return {
    manifestPath: manifest ? resolve(manifest) : undefined,
    buildId,
    verifyOnly,
    registerOnly,
    serverUrl: serverUrl.origin,
    reportPath: options.get("--report"),
  };
};

const requiredEnv = (name: string) => {
  const value = Deno.env.get(name)?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
};

const delay = (milliseconds: number) =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

const retry = async <T>(
  operation: () => Promise<T>,
  label: string,
): Promise<T> => {
  let lastError: unknown;
  let attempts = 0;
  for (let attempt = 0; attempt < RETRY_COUNT; attempt++) {
    attempts++;
    try {
      return await operation();
    } catch (error) {
      lastError = error;
      if (
        typeof error === "object" && error !== null && "permanent" in error &&
        error.permanent === true
      ) break;
      if (
        error instanceof Error && /HTTP 4\d\d/.test(error.message) &&
        !/HTTP (408|429)/.test(error.message)
      ) break;
      if (attempt + 1 === RETRY_COUNT) break;
      await delay(Math.min(1_000 * 2 ** attempt, 16_000));
    }
  }
  throw new Error(
    `${label} failed after ${attempts} attempt(s): ${
      lastError instanceof Error ? lastError.message : String(lastError)
    }`,
    {
      cause: lastError,
    },
  );
};

export const digestFile = async (filePath: string) => {
  const stat = await Deno.stat(filePath);
  const sha256 = createHash("sha256");
  const md5 = createHash("md5");
  const file = await Deno.open(filePath, { read: true });
  try {
    const buffer = new Uint8Array(1024 * 1024);
    while (true) {
      const read = await file.read(buffer);
      if (read === null) break;
      const chunk = buffer.subarray(0, read);
      sha256.update(chunk);
      md5.update(chunk);
    }
  } finally {
    file.close();
  }
  return {
    fileSize: stat.size,
    sha256: sha256.digest("hex"),
    md5: md5.digest("hex"),
  };
};

const validateManifest = async (
  manifestPath: string,
  manifest: PublishManifest,
) => {
  if (
    manifest.schemaVersion !== 1 || !manifest.buildId ||
    !manifest.itemIdentifier
  ) {
    throw new Error(
      "Manifest must have schemaVersion 1, buildId, and itemIdentifier",
    );
  }
  if (
    !manifest.artifacts.length ||
    manifest.artifacts.some((artifact) => !artifact.path)
  ) {
    throw new Error("Manifest must include local paths for every artifact");
  }
  if (
    manifest.artifacts.some((artifact) =>
      artifact.fileName === "artifact-manifest.json"
    )
  ) {
    throw new Error(
      "artifact-manifest.json is reserved for the sanitized IA build manifest",
    );
  }
  const names = new Set<string>();
  const baseDirectory = dirname(manifestPath);
  for (const artifact of manifest.artifacts) {
    if (names.has(artifact.fileName)) {
      throw new Error(`Duplicate artifact name: ${artifact.fileName}`);
    }
    if (
      basename(artifact.path) !== artifact.fileName ||
      !Number.isSafeInteger(artifact.fileSize) || artifact.fileSize <= 0 ||
      !/^[a-fA-F0-9]{64}$/.test(artifact.sha256) ||
      !/^[a-fA-F0-9]{32}$/.test(artifact.md5)
    ) {
      throw new Error(`Invalid local artifact entry: ${artifact.fileName}`);
    }
    names.add(artifact.fileName);
    const fullPath = resolve(baseDirectory, artifact.path);
    const actual = await digestFile(fullPath);
    if (
      actual.fileSize !== artifact.fileSize ||
      actual.sha256 !== artifact.sha256.toLowerCase() ||
      actual.md5 !== artifact.md5.toLowerCase()
    ) {
      throw new Error(
        `Local file does not match manifest: ${artifact.fileName}`,
      );
    }
    artifact.path = fullPath;
    artifact.sha256 = actual.sha256;
    artifact.md5 = actual.md5;
  }
  return manifest;
};

const apiRequest = async (
  url: URL,
  token: string,
  init: RequestInit = {},
): Promise<Response> => {
  const response = await fetch(url, {
    ...init,
    redirect: "error",
    headers: {
      Authorization: `Bearer ${token}`,
      ...(init.headers ?? {}),
    },
    signal: AbortSignal.timeout(60_000),
  });
  if (!response.ok) {
    const detail = await response.text();
    const error = new Error(
      `Archive API returned HTTP ${response.status}: ${detail}`,
    );
    if (
      /no such (table|column)|must be configured|Server misconfigured/i.test(
        detail,
      )
    ) {
      throw Object.assign(error, { permanent: true });
    }
    throw error;
  }
  if (!response.headers.get("content-type")?.includes("application/json")) {
    const detail = (await response.text()).slice(0, 2000);
    throw Object.assign(
      new Error(
        `Archive API returned non-JSON HTTP ${response.status}: ${detail}`,
      ),
      { permanent: true },
    );
  }
  return response;
};

export const registerBuild = async (
  serverUrl: string,
  token: string,
  manifest: PublishManifest,
): Promise<BuildResponse> => {
  const { artifacts, ...buildFields } = manifest;
  const body = {
    ...buildFields,
    artifacts: artifacts.map(({ path: _path, ...artifact }) => artifact),
  };
  const response = await retry(async () => {
    const result = await apiRequest(
      new URL("/archive/builds", serverUrl),
      token,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      },
    );
    return await result.json() as BuildResponse;
  }, "build registration");
  if (
    response.buildId !== manifest.buildId ||
    response.itemIdentifier !== manifest.itemIdentifier
  ) {
    throw new Error("Archive API returned an unexpected build identity");
  }
  if (
    manifest.publishedAt !== undefined &&
    response.publishedAt !== manifest.publishedAt
  ) {
    throw new Error("Archive API server publication timestamp mismatch");
  }
  for (const artifact of manifest.artifacts) {
    const registered = response.artifacts.find((item) =>
      item.fileName === artifact.fileName
    );
    if (
      !registered || registered.storageKey !== artifact.fileName ||
      registered.fileSize !== artifact.fileSize
    ) {
      throw new Error(
        `Archive API returned an unexpected artifact mapping for ${artifact.fileName}`,
      );
    }
    const download = new URL(registered.downloadUrl);
    if (
      download.origin !== serverUrl || !download.pathname.startsWith("/bin/")
    ) {
      throw new Error(
        `Archive API returned a non-canonical download URL for ${artifact.fileName}`,
      );
    }
  }
  return response;
};

const verifyRemoteBuild = async (
  serverUrl: string,
  token: string,
  buildId: string,
) => {
  const verifyAttempts = Math.max(
    1,
    Math.min(24, Number(Deno.env.get("ARCHIVE_VERIFY_ATTEMPTS") ?? "4") || 4),
  );
  const verifyDelay = Math.max(
    0,
    Math.min(
      60_000,
      Number(Deno.env.get("ARCHIVE_VERIFY_DELAY_MS") ?? "15000") || 0,
    ),
  );
  let verification: BuildResponse | undefined;
  for (let attempt = 0; attempt < verifyAttempts; attempt++) {
    verification = await retry(async () => {
      const result = await apiRequest(
        new URL(
          `/archive/builds/${encodeURIComponent(buildId)}/verify`,
          serverUrl,
        ),
        token,
        { method: "POST" },
      );
      return await result.json() as BuildResponse;
    }, "archive verification");
    if (verification.status === "ready" || verification.status === "failed") {
      break;
    }
    if (attempt + 1 < verifyAttempts) await delay(verifyDelay);
  }
  return verification!;
};

const reconcile = async (serverUrl: string, buildId: string) => {
  const token = Deno.env.get("ARCHIVE_UPLOAD_TOKEN")?.trim() ||
    Deno.env.get("FILE_SERVER_TOKEN")?.trim() ||
    requiredEnv("UPLOAD_TOKEN");
  const verification = await verifyRemoteBuild(serverUrl, token, buildId);
  return {
    schemaVersion: 1 as const,
    buildId: verification.buildId,
    itemIdentifier: verification.itemIdentifier,
    target: verification.target ?? null,
    publishedAt: verification.publishedAt,
    channel: verification.channel,
    commit: verification.commit,
    folderPath: verification.folderPath,
    status: verification.status,
    diagnostics: verification.diagnostics,
    artifacts: verification.artifacts,
  };
};

interface RemoteFile {
  name: string;
  size?: string | number;
  md5?: string;
}

export const readRemoteFile = async (
  itemIdentifier: string,
  fileName: string,
): Promise<RemoteFile | undefined> => {
  const response = await fetch(
    new URL(
      `/metadata/${encodeURIComponent(itemIdentifier)}`,
      IA_METADATA_ORIGIN,
    ),
    {
      signal: AbortSignal.timeout(15_000),
    },
  );
  if (response.status === 404) {
    await response.body?.cancel();
    return undefined;
  }
  if (!response.ok) {
    const detail = await response.text();
    throw new Error(`IA metadata returned HTTP ${response.status}: ${detail}`);
  }
  const metadata = await response.json() as {
    metadata?: { identifier?: string; uploader?: string };
    files?: RemoteFile[];
  };
  if (
    !metadata.metadata?.identifier &&
    (!metadata.files || metadata.files.length === 0)
  ) {
    return undefined;
  }
  if (metadata.metadata?.identifier !== itemIdentifier) {
    throw Object.assign(new Error("IA metadata identifier mismatch"), {
      permanent: true,
    });
  }
  if (!Array.isArray(metadata.files)) {
    throw new Error("IA item metadata did not include a file inventory");
  }
  const uploader = requiredEnv("IA_UPLOADER");
  if (metadata.metadata.uploader !== uploader) {
    throw Object.assign(new Error("IA item belongs to a different uploader"), {
      permanent: true,
    });
  }
  return metadata.files?.find((file) => file.name === fileName);
};

export const skipIfAlreadyUploaded = async (
  itemIdentifier: string,
  fileName: string,
  expected: { fileSize: number; md5: string },
) => {
  const remote = await readRemoteFile(itemIdentifier, fileName);
  if (!remote) return false;
  if (remote.size === undefined || !remote.md5) {
    throw new Error(
      `IA has not indexed size and MD5 for existing file ${fileName}`,
    );
  }
  if (
    Number(remote.size) === expected.fileSize &&
    remote.md5.toLowerCase() === expected.md5
  ) {
    return true;
  }
  throw Object.assign(
    new Error(
      `IA already has different bytes for immutable build file ${fileName}`,
    ),
    { permanent: true },
  );
};

export const allowedIaUploadUrl = (value: string, itemIdentifier?: string) => {
  const url = new URL(value);
  const allowedHost = url.hostname === "s3.us.archive.org" ||
    (itemIdentifier !== undefined &&
      url.hostname === `${itemIdentifier}.s3.us.archive.org`);
  return url.protocol === "https:" && !url.username && !url.password &&
    url.port === "" && allowedHost;
};

export const streamUpload = async (
  filePath: string,
  destination: URL,
  fileSize: number,
  accessKey: string,
  secret: string,
  firstFile: boolean,
  manifest: PublishManifest,
) => {
  let url = destination;
  for (let redirects = 0; redirects <= 3; redirects++) {
    if (!allowedIaUploadUrl(url.href, manifest.itemIdentifier)) {
      throw new Error("IA upload redirected to a non-IA S3 host");
    }
    const file = await Deno.open(filePath, { read: true });
    let response: Response;
    try {
      response = await fetch(url, {
        method: "PUT",
        redirect: "manual",
        headers: {
          Authorization: `LOW ${accessKey}:${secret}`,
          "content-type": "application/octet-stream",
          "content-length": String(fileSize),
          "x-archive-queue-derive": "0",
          ...(firstFile
            ? {
              "x-archive-auto-make-bucket": "1",
              "x-archive-meta-mediatype": "software",
              "x-archive-meta-title": `Clash Nyanpasu ${manifest.channel} ${
                manifest.tag ?? manifest.commit
              }`,
              "x-archive-meta-source": IA_SOURCE,
              "x-archive-meta-homepage": IA_HOMEPAGE,
              "x-archive-meta-licenseurl": IA_LICENSE_URL,
            }
            : {}),
        },
        body: file.readable,
        signal: AbortSignal.timeout(30 * 60_000),
      });
    } finally {
      try {
        file.close();
      } catch {
        // Fetch may have drained and closed the Deno file stream already.
      }
    }
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location || redirects === 3) {
        throw new Error("IA returned an invalid upload redirect");
      }
      url = new URL(location, url);
      continue;
    }
    if ([200, 201, 204].includes(response.status)) {
      await response.body?.cancel();
      return;
    }
    const body = await response.text();
    const error = new Error(
      `IA upload returned HTTP ${response.status}: ${body}`,
    );
    if (
      response.status === 503 || response.status === 429 ||
      response.status >= 500
    ) throw error;
    throw Object.assign(error, { permanent: true });
  }
  throw new Error("Too many IA upload redirects");
};

const uploadOne = async (
  itemIdentifier: string,
  file: { path: string; fileName: string; fileSize: number; md5: string },
  accessKey: string,
  secret: string,
  firstFile: boolean,
  manifest: PublishManifest,
) => {
  if (
    await retry(
      () => skipIfAlreadyUploaded(itemIdentifier, file.fileName, file),
      `IA metadata check for ${file.fileName}`,
    )
  ) return "skipped" as const;
  const destination = new URL(
    `${encodeURIComponent(itemIdentifier)}/${
      encodeURIComponent(file.fileName)
    }`,
    `${IA_S3_ORIGIN}/`,
  );
  await retry(
    () =>
      streamUpload(
        file.path,
        destination,
        file.fileSize,
        accessKey,
        secret,
        firstFile,
        manifest,
      ),
    `IA upload for ${file.fileName}`,
  );
  return "uploaded" as const;
};

const createSanitizedManifestFile = async (manifest: PublishManifest) => {
  const contents = JSON.stringify(
    {
      schemaVersion: 1,
      buildId: manifest.buildId,
      itemIdentifier: manifest.itemIdentifier,
      channel: manifest.channel,
      commit: manifest.commit,
      publishedAt: manifest.publishedAt,
      tag: manifest.tag,
      target: manifest.target ?? null,
      folderPath: manifest.folderPath,
      artifacts: manifest.artifacts.map(({ path: _path, ...artifact }) =>
        artifact
      ),
    },
    null,
    2,
  );
  const path = await Deno.makeTempFile({
    prefix: "nyanpasu-archive-manifest-",
    suffix: ".json",
  });
  await Deno.writeTextFile(path, contents);
  const digest = await digestFile(path);
  return { path, fileName: "artifact-manifest.json", ...digest };
};

const publish = async (
  manifestPath: string,
  serverUrl: string,
  registerOnly = false,
) => {
  const token = Deno.env.get("ARCHIVE_UPLOAD_TOKEN")?.trim() ||
    Deno.env.get("FILE_SERVER_TOKEN")?.trim() ||
    requiredEnv("UPLOAD_TOKEN");
  const accessKey = registerOnly ? "" : requiredEnv("IA_ACCESS_KEY");
  const secret = registerOnly ? "" : requiredEnv("IA_SECRET_KEY");
  const itemPrefix = requiredEnv("IA_ITEM_PREFIX");
  const manifest = await validateManifest(
    manifestPath,
    JSON.parse(await Deno.readTextFile(manifestPath)) as PublishManifest,
  );
  if (!manifest.itemIdentifier.startsWith(`${itemPrefix}-`)) {
    throw new Error("itemIdentifier is outside IA_ITEM_PREFIX");
  }
  const registered = await registerBuild(serverUrl, token, manifest);
  if (registered.status === "failed") {
    throw new Error(
      `Build registration is failed: ${registered.diagnostics.join("; ")}`,
    );
  }
  const registeredManifest = {
    ...manifest,
    publishedAt: registered.publishedAt ?? manifest.publishedAt,
  };
  if (registerOnly) {
    return {
      schemaVersion: 1 as const,
      ...registered,
      target: manifest.target ?? null,
      channel: manifest.channel,
      commit: manifest.commit,
      folderPath: manifest.folderPath,
      registrationOnly: true,
      uploads: [],
    };
  }

  const sanitized = await createSanitizedManifestFile(registeredManifest);
  const uploads: Array<{ fileName: string; status: "uploaded" | "skipped" }> =
    [];
  try {
    uploads.push({
      fileName: sanitized.fileName,
      status: await uploadOne(
        registeredManifest.itemIdentifier,
        sanitized,
        accessKey,
        secret,
        true,
        registeredManifest,
      ),
    });
    for (const artifact of manifest.artifacts) {
      uploads.push({
        fileName: artifact.fileName,
        status: await uploadOne(
          registeredManifest.itemIdentifier,
          artifact,
          accessKey,
          secret,
          false,
          registeredManifest,
        ),
      });
    }
  } finally {
    await Deno.remove(sanitized.path).catch(() => undefined);
  }

  const verification = await verifyRemoteBuild(
    serverUrl,
    token,
    registeredManifest.buildId,
  );
  if (
    registeredManifest.publishedAt !== undefined &&
    verification.publishedAt !== registeredManifest.publishedAt
  ) {
    throw new Error("Archive API server publication timestamp mismatch");
  }

  return {
    schemaVersion: 1 as const,
    buildId: registeredManifest.buildId,
    itemIdentifier: registeredManifest.itemIdentifier,
    target: registeredManifest.target ?? null,
    publishedAt: registeredManifest.publishedAt,
    channel: registeredManifest.channel,
    commit: registeredManifest.commit,
    folderPath: registeredManifest.folderPath,
    status: verification.status,
    diagnostics: verification.diagnostics,
    artifacts: verification.artifacts,
    uploads,
  };
};

export const runArchivePublish = async (args: string[]): Promise<number> => {
  let reportPath: string | undefined;
  let reportIdentity: Record<string, unknown> = {};
  try {
    const options = parseOptions(args);
    reportPath = options.reportPath;
    if (options.manifestPath) {
      try {
        const manifest = JSON.parse(
          await Deno.readTextFile(options.manifestPath),
        ) as PublishManifest;
        reportIdentity = {
          schemaVersion: 1,
          buildId: manifest.buildId,
          itemIdentifier: manifest.itemIdentifier,
          target: manifest.target ?? null,
          publishedAt: manifest.publishedAt,
          channel: manifest.channel,
          commit: manifest.commit,
          folderPath: manifest.folderPath,
        };
      } catch {
        // Keep a minimal failure report when the supplied manifest cannot be read.
      }
    } else if (options.buildId) {
      reportIdentity = { schemaVersion: 1, buildId: options.buildId };
    }
    const report = options.verifyOnly
      ? await reconcile(options.serverUrl, options.buildId!)
      : await publish(
        options.manifestPath!,
        options.serverUrl,
        options.registerOnly,
      );
    if (reportPath) {
      await Deno.writeTextFile(reportPath, JSON.stringify(report, null, 2));
    }
    console.log(JSON.stringify(report, null, 2));
    if (report.status === "failed") return 1;
    if (options.registerOnly) return 0;
    return report.status === "ready" ? 0 : 2;
  } catch (error) {
    const failure = {
      ...reportIdentity,
      status: "failed",
      error: error instanceof Error ? error.message : String(error),
    };
    if (reportPath) {
      await Deno.writeTextFile(reportPath, JSON.stringify(failure, null, 2));
    }
    console.error(JSON.stringify(failure, null, 2));
    return 1;
  }
};

if (import.meta.main) Deno.exitCode = await runArchivePublish(Deno.args);
