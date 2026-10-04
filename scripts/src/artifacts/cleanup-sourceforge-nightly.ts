import * as path from "jsr:@std/path";
import {
  sftpQuote,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from "./sourceforge.ts";

const MANAGED_BUILD = /^([0-9]+)-([0-9]+)-([a-f0-9]{40})$/;
const SAFE_FILE = /^[A-Za-z0-9][A-Za-z0-9 _.,=+#@()-]*$/;
const EXPECTED_TARGETS = [
  "windows-x86_64",
  "windows-aarch64",
  "linux-x86_64",
  "linux-aarch64",
  "macos-x86_64",
  "macos-aarch64",
] as const;

export function parseManagedNightlyDirectories(listing: string): string[] {
  return listing.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
    .filter((name) => {
      if (!MANAGED_BUILD.test(name)) {
        throw new Error(`Unexpected entry under managed nightly root: ${name}`);
      }
      return true;
    });
}

function compareRunIdentity(a: string, b: string): number {
  const left = a.match(MANAGED_BUILD)!;
  const right = b.match(MANAGED_BUILD)!;
  const runDelta = BigInt(left[1]) - BigInt(right[1]);
  if (runDelta !== 0n) return runDelta < 0n ? -1 : 1;
  const attemptDelta = BigInt(left[2]) - BigInt(right[2]);
  return attemptDelta < 0n ? -1 : attemptDelta > 0n ? 1 : 0;
}

export function selectNightlyCleanupTargets(
  directories: readonly string[],
  promotedBuildId: string,
): string[] | null {
  if (!MANAGED_BUILD.test(promotedBuildId)) {
    throw new Error("Invalid promoted nightly build id");
  }
  if (!directories.includes(promotedBuildId)) {
    throw new Error("Promoted nightly directory is missing");
  }
  const newest = directories.toSorted(compareRunIdentity).at(-1);
  if (newest !== promotedBuildId) return null;
  return directories.filter((directory) => directory !== promotedBuildId);
}

export async function isArchiveBuildReady(
  buildId: string,
  token: string,
  fetcher: typeof fetch = fetch,
): Promise<boolean> {
  let response: Response | undefined;
  try {
    response = await fetcher(
      `https://archive.nyanpasu.org/archive/builds/${
        encodeURIComponent(buildId)
      }`,
      {
        headers: { authorization: `Bearer ${token}` },
        redirect: "manual",
        signal: AbortSignal.timeout(8_000),
      },
    );
    if (!response?.ok || response.headers.has("location")) return false;
    const body = await response.json();
    return typeof body === "object" && body !== null &&
      body.status === "ready" && body.buildId === buildId;
  } catch {
    return false;
  } finally {
    await response?.body?.cancel().catch(() => {});
  }
}

export async function archiveReadyTargets(
  globalBuildId: string,
  token: string,
  fetcher: typeof fetch = fetch,
): Promise<{ ready: boolean; missing: string[] }> {
  const match = globalBuildId.match(MANAGED_BUILD);
  if (!match) throw new Error("Invalid global nightly build id");
  const missing: string[] = [];
  for (const target of EXPECTED_TARGETS) {
    const targetBuildId = `${globalBuildId}-${target}`;
    if (!await isArchiveBuildReady(targetBuildId, token, fetcher)) {
      missing.push(target);
    }
  }
  return { ready: missing.length === 0, missing };
}

function requiredEnv(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    throw new Error(
      `${name} is required when SourceForge nightly cleanup is enabled`,
    );
  }
  return value;
}

async function runSftp(
  key: string,
  knownHosts: string,
  username: string,
  batch: string,
): Promise<{ success: boolean; stdout: string; stderr: string }> {
  const command = new Deno.Command("sftp", {
    args: [
      "-i",
      key,
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "-o",
      `UserKnownHostsFile=${knownHosts}`,
      "-o",
      "StrictHostKeyChecking=yes",
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
  return { success: status.success, stdout, stderr };
}

async function main(): Promise<void> {
  const reportPath = path.resolve(
    Deno.args[0] ?? "sourceforge-cleanup-report.json",
  );
  const project = requiredEnv("SOURCEFORGE_PROJECT");
  const username = requiredEnv("SOURCEFORGE_USERNAME");
  const buildId = requiredEnv("SOURCEFORGE_BUILD_ID");
  validateSourceforgeProject(project);
  validateSourceforgeUsername(username);
  if (!MANAGED_BUILD.test(buildId)) {
    throw new Error(
      "SOURCEFORGE_BUILD_ID is not a full managed nightly identity",
    );
  }
  const root = `/home/frs/project/${project}/nightly`;
  const report: {
    schemaVersion: 1;
    status: "success" | "skipped-stale" | "failure";
    project: string;
    buildId: string;
    deleted: string[];
    retained: Array<{ buildId: string; reason: string }>;
    error: string | null;
  } = {
    schemaVersion: 1,
    status: "failure",
    project,
    buildId,
    deleted: [],
    retained: [],
    error: null,
  };
  const temp = await Deno.makeTempDir({ prefix: "sourceforge-cleanup-" });
  try {
    const key = path.join(temp, "id_sourceforge");
    const knownHosts = path.join(temp, "known_hosts");
    await Deno.writeTextFile(key, requiredEnv("SOURCEFORGE_SSH_KEY"), {
      mode: 0o600,
    });
    await Deno.writeTextFile(
      knownHosts,
      requiredEnv("SOURCEFORGE_KNOWN_HOSTS"),
    );
    if (Deno.build.os === "windows") {
      const user = requiredEnv("USERNAME");
      if (!/^[A-Za-z0-9._-]+$/.test(user)) {
        throw new Error("Invalid Windows runner username");
      }
      const acl = await new Deno.Command("icacls", {
        args: [key, "/inheritance:r", "/grant:r", `${user}:(R)`],
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
    } else await Deno.chmod(key, 0o600);

    const listing = await runSftp(
      key,
      knownHosts,
      username,
      `@cd ${sftpQuote(root)}\n@ls -1\n`,
    );
    if (!listing.success) {
      throw new Error(
        `Could not list managed SourceForge nightly directories: ${
          listing.stderr || listing.stdout
        }`,
      );
    }
    const directories = parseManagedNightlyDirectories(listing.stdout);
    const cleanupTargets = selectNightlyCleanupTargets(directories, buildId);
    if (cleanupTargets === null) {
      report.status = "skipped-stale";
    } else {
      for (const directory of cleanupTargets) {
        if (Deno.env.get("IA_ITEM_PREFIX")?.trim()) {
          const token = Deno.env.get("ARCHIVE_UPLOAD_TOKEN") ||
            Deno.env.get("FILE_SERVER_TOKEN") || Deno.env.get("UPLOAD_TOKEN");
          if (!token) {
            report.retained.push({
              buildId: directory,
              reason: "Internet Archive verification token is not configured",
            });
            continue;
          }
          const archive = await archiveReadyTargets(directory, token);
          if (!archive.ready) {
            report.retained.push({
              buildId: directory,
              reason: `Internet Archive targets are not ready: ${
                archive.missing.join(", ")
              }`,
            });
            continue;
          }
        }
        const filesResult = await runSftp(
          key,
          knownHosts,
          username,
          `@cd ${sftpQuote(`${root}/${directory}`)}\n@ls -1\n`,
        );
        if (!filesResult.success) {
          throw new Error(
            `Could not inspect managed nightly directory ${directory}: ${
              filesResult.stderr || filesResult.stdout
            }`,
          );
        }
        const files = filesResult.stdout.split(/\r?\n/).map((line) =>
          line.trim()
        ).filter(Boolean);
        if (
          files.some((file) =>
            !SAFE_FILE.test(file) || file === "." || file === ".."
          )
        ) {
          throw new Error(
            `Unexpected file in managed nightly directory ${directory}`,
          );
        }
        const commands = files.map((file) =>
          `@rm ${sftpQuote(`${root}/${directory}/${file}`)}`
        );
        commands.push(
          `@cd ${sftpQuote(root)}`,
          `@rmdir ${sftpQuote(directory)}`,
        );
        const removal = await runSftp(
          key,
          knownHosts,
          username,
          `${commands.join("\n")}\n`,
        );
        if (!removal.success) {
          throw new Error(
            `Could not remove managed nightly build ${directory}: ${
              removal.stderr || removal.stdout
            }`,
          );
        }
        report.deleted.push(directory);
      }
      report.status = "success";
    }
  } catch (error) {
    report.error = error instanceof Error ? error.message : String(error);
  } finally {
    await Deno.remove(temp, { recursive: true });
    await Deno.mkdir(path.dirname(reportPath), { recursive: true });
    await Deno.writeTextFile(
      reportPath,
      `${JSON.stringify(report, null, 2)}\n`,
    );
  }
  if (report.status === "failure") {
    throw new Error(report.error ?? "SourceForge cleanup failed");
  }
  console.log(
    `SourceForge nightly cleanup ${report.status}; removed ${report.deleted.length} old build(s)`,
  );
}

if (import.meta.main) await main();
