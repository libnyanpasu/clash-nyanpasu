import * as path from "jsr:@std/path";
import { Octokit } from "npm:octokit";
import {
  guardReleaseMirrorUpload,
  sourceforgeDownloadUrl,
  sourceforgeUploadBatch,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from "./sourceforge.ts";

interface PublishManifest {
  channel: "release" | "nightly";
  target: string;
  buildId: string;
  publishedAt?: string;
  artifacts: Array<
    { fileName: string; fileSize: number; sha256: string; path: string }
  >;
}

interface SourceforgeReport {
  schemaVersion: number;
  status: "failure" | "uploaded";
  project: string | null;
  channel: string | null;
  target: string | null;
  buildId: string | null;
  publishedAt?: string;
  remotePath: string | null;
  artifacts: Array<
    { fileName: string; fileSize: number; sha256: string; url: string }
  >;
  error: string | null;
}

function env(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    throw new Error(
      `${name} is required when SourceForge mirroring is enabled`,
    );
  }
  return value;
}

async function runSftp(
  keyPath: string,
  knownHostsPath: string,
  batch: string,
): Promise<void> {
  const username = env("SOURCEFORGE_USERNAME");
  validateSourceforgeUsername(username);
  const command = new Deno.Command("sftp", {
    args: [
      "-i",
      keyPath,
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "-o",
      `UserKnownHostsFile=${knownHostsPath}`,
      "-o",
      "StrictHostKeyChecking=yes",
      "-o",
      "ServerAliveInterval=30",
      "-o",
      "ConnectTimeout=30",
      "-b",
      "-",
      `${username}@frs.sourceforge.net`,
    ],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  }).spawn();
  const writer = command.stdin.getWriter();
  await writer.write(new TextEncoder().encode(batch));
  await writer.close();
  const [status, stdout, stderr] = await Promise.all([
    command.status,
    new Response(command.stdout).text(),
    new Response(command.stderr).text(),
  ]);
  if (!status.success) {
    throw new Error(`sftp failed (${status.code}): ${stderr || stdout}`);
  }
}

async function loadPreviousReleaseMirror(
  releaseTag: string,
): Promise<unknown | null> {
  const token = env("GITHUB_TOKEN");
  const repository = env("GITHUB_REPOSITORY");
  const [owner, repo, extra] = repository.split("/");
  if (!owner || !repo || extra) {
    throw new Error("GITHUB_REPOSITORY must be owner/repo");
  }
  const github = new Octokit({ auth: token });
  const { data: release } = await github.rest.repos.getReleaseByTag({
    owner,
    repo,
    tag: releaseTag,
  });
  if (release.tag_name !== releaseTag) {
    throw new Error("GitHub release tag does not match SourceForge release");
  }
  const assets = await github.paginate(github.rest.repos.listReleaseAssets, {
    owner,
    repo,
    release_id: release.id,
    per_page: 100,
  });
  const mirrors = assets.filter((asset) =>
    asset.name === "sourceforge-mirrors.json"
  );
  if (mirrors.length > 1) {
    throw new Error("GitHub release has multiple SourceForge mirror manifests");
  }
  const asset = mirrors[0];
  if (!asset) return null;
  const response = await fetch(
    `https://api.github.com/repos/${owner}/${repo}/releases/assets/${asset.id}`,
    {
      headers: {
        accept: "application/octet-stream",
        authorization: `Bearer ${token}`,
        "x-github-api-version": "2022-11-28",
      },
      signal: AbortSignal.timeout(30_000),
    },
  );
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(
      `Could not read existing SourceForge metadata: HTTP ${response.status}`,
    );
  }
  try {
    return JSON.parse(await response.text()) as unknown;
  } catch (error) {
    throw new Error("Existing SourceForge release metadata is not valid JSON", {
      cause: error,
    });
  }
}

async function main(): Promise<void> {
  const manifestPath = Deno.args[0];
  const reportPath = Deno.args[1];
  if (!manifestPath || !reportPath) {
    throw new Error("Usage: sourceforge:upload <manifest.json> <report.json>");
  }
  let report: SourceforgeReport = {
    schemaVersion: 1,
    status: "failure",
    project: null,
    channel: null,
    target: null,
    buildId: null,
    remotePath: null,
    artifacts: [],
    error: "SourceForge upload did not complete",
  };
  try {
    const manifest = JSON.parse(
      await Deno.readTextFile(manifestPath),
    ) as PublishManifest;
    const project = env("SOURCEFORGE_PROJECT");
    const buildId = env("SOURCEFORGE_BUILD_ID");
    const channel = env("SOURCEFORGE_CHANNEL");
    const releaseTag = Deno.env.get("SOURCEFORGE_RELEASE_TAG") ?? "";
    validateSourceforgeProject(project);
    if (
      channel !== manifest.channel ||
      (channel !== "release" && channel !== "nightly")
    ) {
      throw new Error(
        "SourceForge channel does not match the prepared manifest",
      );
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(buildId)) {
      throw new Error("Invalid SourceForge build id");
    }
    if (channel === "release" && (!releaseTag || buildId !== releaseTag)) {
      throw new Error(
        "Release SourceForge build id must equal the release tag",
      );
    }
    if (!manifest.target || manifest.artifacts.length === 0) {
      throw new Error("SourceForge manifest has no target or artifacts");
    }
    const remotePath = channel === "nightly"
      ? `nightly/${buildId}`
      : `releases/${releaseTag}`;
    const artifacts = manifest.artifacts.map((artifact) => {
      if (
        !Number.isSafeInteger(artifact.fileSize) || artifact.fileSize <= 0 ||
        !/^[a-f0-9]{64}$/.test(artifact.sha256)
      ) {
        throw new Error(`Invalid size or SHA-256 for ${artifact.fileName}`);
      }
      return {
        fileName: artifact.fileName,
        fileSize: artifact.fileSize,
        sha256: artifact.sha256,
        url: sourceforgeDownloadUrl(project, remotePath, artifact.fileName),
      };
    });
    report = {
      ...report,
      project,
      channel,
      target: manifest.target,
      buildId,
      remotePath,
      ...(manifest.publishedAt === undefined
        ? {}
        : { publishedAt: manifest.publishedAt }),
      artifacts,
    };

    const tempDir = await Deno.makeTempDir({ prefix: "sourceforge-upload-" });
    try {
      const keyPath = path.join(tempDir, "id_sourceforge");
      const knownHostsPath = path.join(tempDir, "known_hosts");
      await Deno.writeTextFile(keyPath, env("SOURCEFORGE_SSH_KEY"), {
        mode: 0o600,
      });
      await Deno.writeTextFile(knownHostsPath, env("SOURCEFORGE_KNOWN_HOSTS"));
      if (Deno.build.os === "windows") {
        const runnerUser = env("USERNAME");
        if (!/^[A-Za-z0-9._-]+$/.test(runnerUser)) {
          throw new Error("Invalid Windows runner username");
        }
        const acl = await new Deno.Command("icacls", {
          args: [keyPath, "/inheritance:r", "/grant:r", `${runnerUser}:(R)`],
          stdout: "piped",
          stderr: "piped",
        }).output();
        if (!acl.success) {
          throw new Error(
            `Could not secure Windows SSH key ACL: ${
              new TextDecoder().decode(acl.stderr)
            }`,
          );
        }
      } else {
        await Deno.chmod(keyPath, 0o600);
      }

      const upload = async () => {
        const batch = sourceforgeUploadBatch(
          project,
          remotePath,
          manifest.artifacts,
          Deno.build.os === "windows",
        );
        let lastError: unknown;
        for (let attempt = 1; attempt <= 4; attempt++) {
          try {
            await runSftp(keyPath, knownHostsPath, batch);
            lastError = undefined;
            break;
          } catch (error) {
            lastError = error;
            if (attempt < 4) {
              await new Promise((resolve) =>
                setTimeout(resolve, attempt * 5_000)
              );
            }
          }
        }
        if (lastError) throw lastError;
      };
      if (channel === "release") {
        await guardReleaseMirrorUpload(
          project,
          releaseTag,
          manifest,
          () => loadPreviousReleaseMirror(releaseTag),
          upload,
        );
      } else {
        await upload();
      }
    } finally {
      await Deno.remove(tempDir, { recursive: true });
    }
    report.status = "uploaded";
    report.error = null;
  } catch (error) {
    report.error = error instanceof Error ? error.message : String(error);
    await Deno.writeTextFile(
      reportPath,
      `${JSON.stringify(report, null, 2)}\n`,
    );
    throw error;
  }
  await Deno.writeTextFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `Uploaded ${report.artifacts.length} SourceForge assets to ${report.remotePath}`,
  );
}

if (import.meta.main) await main();
