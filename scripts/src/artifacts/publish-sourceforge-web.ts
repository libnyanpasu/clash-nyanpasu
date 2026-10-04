import * as path from "jsr:@std/path";
import {
  sourceforgeWebUploadBatch,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from "./sourceforge.ts";

const CHANNEL_FILES = {
  release: ["update.json", "update-beta.json"],
  nightly: ["update-nightly.json"],
} as const;

function env(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    throw new Error(
      `${name} is required when SourceForge web manifests are enabled`,
    );
  }
  return value;
}

async function sftp(
  keyPath: string,
  knownHostsPath: string,
  batch: string,
): Promise<void> {
  const user = env("SOURCEFORGE_USERNAME");
  validateSourceforgeUsername(user);
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
      `${user}@web.sourceforge.net`,
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
    throw new Error(
      `SourceForge web SFTP failed (${status.code}): ${stderr || stdout}`,
    );
  }
}

async function fetchAndVerify(url: string, expected: string): Promise<void> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= 8; attempt++) {
    try {
      const response = await fetch(url, {
        redirect: "follow",
        signal: AbortSignal.timeout(30_000),
      });
      if (
        response.ok &&
        response.headers.get("content-type")?.toLowerCase().includes(
          "text/html",
        )
      ) {
        await response.body?.cancel();
        throw new Error(`Manifest URL returned HTML: ${url}`);
      }
      if (response.ok) {
        const published = await response.text();
        JSON.parse(published);
        if (published === expected) return;
        lastError = new Error(
          `Published SourceForge manifest differs from expected content: ${url}`,
        );
      } else {
        await response.body?.cancel();
        lastError = new Error(`HTTP ${response.status} from ${url}`);
      }
    } catch (error) {
      lastError = error;
    }
    if (attempt < 8) {
      await new Promise((resolve) =>
        setTimeout(resolve, Math.min(60_000, attempt * 10_000))
      );
    }
  }
  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

async function main(): Promise<void> {
  const inputDir = Deno.args[0];
  const channel = Deno.args[1];
  const reportPath = Deno.args[2] ?? "sourceforge-web-report.json";
  if (!inputDir || (channel !== "release" && channel !== "nightly")) {
    throw new Error(
      "Usage: sourceforge:web-publish <manifest-dir> <release|nightly> [report.json]",
    );
  }
  const project = env("SOURCEFORGE_PROJECT");
  validateSourceforgeProject(project);
  const buildId = env("SOURCEFORGE_BUILD_ID");
  if (!/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(buildId)) {
    throw new Error("Invalid SourceForge build id");
  }
  const root = path.resolve(inputDir);
  const files: Array<
    { path: string; fileName: string; stagedName: string; content: string }
  > = [];
  const expectedFiles = CHANNEL_FILES[channel];
  for (const fileName of expectedFiles) {
    const filePath = path.join(root, fileName);
    try {
      const content = await Deno.readTextFile(filePath);
      JSON.parse(content);
      files.push({
        path: filePath,
        fileName,
        stagedName: `staging-${buildId}-${fileName}`,
        content,
      });
    } catch (error) {
      if (error instanceof Deno.errors.NotFound) {
        throw new Error(`Missing updater manifest ${fileName}`);
      }
      throw new Error(`Invalid updater manifest ${fileName}: ${error}`);
    }
  }

  // Preserve an existing project-owned index. Only add our small listing page when absent.
  const indexUrl = `https://${project}.sourceforge.io/updater/index.html`;
  const indexResponse = await fetch(indexUrl, {
    redirect: "manual",
    signal: AbortSignal.timeout(30_000),
  });
  const addIndex = indexResponse.status === 404;
  if (
    !addIndex && !indexResponse.ok &&
    !(indexResponse.status >= 300 && indexResponse.status < 400)
  ) {
    await indexResponse.body?.cancel();
    throw new Error(
      `Could not inspect existing SourceForge updater index (HTTP ${indexResponse.status})`,
    );
  }
  await indexResponse.body?.cancel();

  const report = {
    schemaVersion: 1,
    status: "failure",
    project,
    buildId,
    manifests: files.map(({ fileName }) => ({
      fileName,
      url: `https://${project}.sourceforge.io/updater/${fileName}`,
    })),
    error: null as string | null,
  };
  const tempDir = await Deno.makeTempDir({ prefix: "sourceforge-web-" });
  try {
    const keyPath = path.join(tempDir, "id_sourceforge");
    const knownHostsPath = path.join(tempDir, "known_hosts");
    await Deno.writeTextFile(keyPath, env("SOURCEFORGE_SSH_KEY"), {
      mode: 0o600,
    });
    await Deno.writeTextFile(knownHostsPath, env("SOURCEFORGE_KNOWN_HOSTS"));
    if (Deno.build.os === "windows") {
      const username = env("USERNAME");
      if (!/^[A-Za-z0-9._-]+$/.test(username)) {
        throw new Error("Invalid Windows runner username");
      }
      const acl = await new Deno.Command("icacls", {
        args: [keyPath, "/inheritance:r", "/grant:r", `${username}:(R)`],
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
    } else await Deno.chmod(keyPath, 0o600);

    if (addIndex) {
      const indexPath = path.join(tempDir, "index.html");
      await Deno.writeTextFile(
        indexPath,
        "<!doctype html><meta charset=utf-8><title>Clash Nyanpasu updater feeds</title><ul><li><a href=update.json>Stable</a></li><li><a href=update-beta.json>Beta</a></li><li><a href=update-nightly.json>Nightly</a></li></ul>\n",
      );
      files.push({
        path: indexPath,
        fileName: "index.html",
        stagedName: `staging-${buildId}-index.html`,
        content: "",
      });
    }
    const batch = sourceforgeWebUploadBatch(
      project,
      files.map(({ path, fileName, stagedName }) => ({
        path,
        fileName,
        stagedName,
      })),
    );
    let lastError: unknown;
    for (let attempt = 1; attempt <= 4; attempt++) {
      try {
        await sftp(keyPath, knownHostsPath, batch);
        lastError = undefined;
        break;
      } catch (error) {
        lastError = error;
        if (attempt < 4) {
          await new Promise((resolve) => setTimeout(resolve, attempt * 5_000));
        }
      }
    }
    if (lastError) throw lastError;

    for (
      const { fileName, content } of files.filter(({ fileName }) =>
        expectedFiles.includes(fileName as never)
      )
    ) {
      const url = `https://${project}.sourceforge.io/updater/${fileName}`;
      await fetchAndVerify(url, content);
    }
    report.status = "success";
  } catch (error) {
    report.error = error instanceof Error ? error.message : String(error);
    await Deno.writeTextFile(
      reportPath,
      `${JSON.stringify(report, null, 2)}\n`,
    );
    throw error;
  } finally {
    await Deno.remove(tempDir, { recursive: true });
  }
  await Deno.writeTextFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `Published and verified ${files.length} SourceForge updater manifests`,
  );
}

if (import.meta.main) await main();
