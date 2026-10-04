import { createHash } from "node:crypto";
import * as path from "jsr:@std/path";
import {
  validateSourceforgeFilename,
  validateSourceforgeRemotePath,
} from "./sourceforge.ts";

export interface SourceforgeArtifact {
  fileName: string;
  fileSize: number;
  sha256: string;
  url: string;
}

export interface SourceforgeUploadReport {
  schemaVersion: 1;
  status: "uploaded";
  project: string;
  channel: "release" | "nightly";
  buildId: string;
  remotePath: string;
  target: string;
  publishedAt?: string;
  artifacts: SourceforgeArtifact[];
}

export const REQUIRED_TARGETS = [
  "windows-x86_64",
  "windows-aarch64",
  "linux-x86_64",
  "linux-aarch64",
  "macos-x86_64",
  "macos-aarch64",
] as const;

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validName(value: unknown): value is string {
  if (
    typeof value !== "string" || value.length > 240 || value !== value.trim()
  ) return false;
  try {
    validateSourceforgeFilename(value);
    return true;
  } catch {
    return false;
  }
}

function validRemotePath(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    validateSourceforgeRemotePath(value);
    return true;
  } catch {
    return false;
  }
}

function validPublicationTime(value: unknown): boolean {
  if (value === undefined) return true;
  if (typeof value !== "string") return false;
  try {
    return new Date(value).toISOString() === value;
  } catch {
    return false;
  }
}

function validateArtifactUrl(
  artifact: SourceforgeArtifact,
  report: SourceforgeUploadReport,
): boolean {
  try {
    const url = new URL(artifact.url);
    const segments = url.pathname.split("/").map(decodeURIComponent);
    return url.protocol === "https:" &&
      url.hostname === "downloads.sourceforge.net" && !url.port &&
      !url.username && !url.password && !url.search && !url.hash &&
      segments.length === 6 &&
      segments.join("/") ===
        `/project/${report.project}/${report.remotePath}/${artifact.fileName}`;
  } catch {
    return false;
  }
}

/** Validate files crossing a CI job boundary before following their URLs. */
export function validateSourceforgeReports(
  reports: unknown[],
  expectedTargets: readonly string[],
): string[] {
  const issues: string[] = [];
  if (
    !expectedTargets.length ||
    new Set(expectedTargets).size !== expectedTargets.length
  ) {
    issues.push("expected targets must be non-empty and unique");
  }
  const seenTargets = new Set<string>();
  const seenAssets = new Set<string>();
  let identity: string | undefined;
  for (const value of reports) {
    if (!record(value)) {
      issues.push("upload report must be an object");
      continue;
    }
    const target = typeof value.target === "string" ? value.target : "(unset)";
    if (!expectedTargets.includes(target) || seenTargets.has(target)) {
      issues.push(`unexpected, missing or duplicate target: ${target}`);
    }
    seenTargets.add(target);
    if (value.schemaVersion !== 1 || value.status !== "uploaded") {
      issues.push(`${target}: upload status is ${String(value.status)}`);
    }
    if (
      typeof value.project !== "string" ||
      !/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(value.project) ||
      !["release", "nightly"].includes(String(value.channel)) ||
      typeof value.buildId !== "string" ||
      !/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(value.buildId) ||
      ["latest", "current", "stable"].includes(value.buildId) ||
      !validRemotePath(value.remotePath) ||
      !validPublicationTime(value.publishedAt) ||
      value.remotePath !==
        `${
          value.channel === "nightly" ? "nightly" : "releases"
        }/${value.buildId}`
    ) {
      issues.push(
        `${target}: invalid project, channel or immutable build path`,
      );
      continue;
    }
    const report = value as unknown as SourceforgeUploadReport;
    const currentIdentity = JSON.stringify([
      report.project,
      report.channel,
      report.buildId,
      report.remotePath,
      report.publishedAt ?? null,
    ]);
    identity ??= currentIdentity;
    if (identity !== currentIdentity) {
      issues.push(`${target}: inconsistent build metadata`);
    }
    if (!Array.isArray(report.artifacts) || report.artifacts.length === 0) {
      issues.push(`${target}: artifact inventory is empty`);
      continue;
    }
    for (const artifact of report.artifacts) {
      if (
        !record(artifact) || !validName(artifact.fileName) ||
        !Number.isSafeInteger(artifact.fileSize) || artifact.fileSize <= 0 ||
        typeof artifact.sha256 !== "string" ||
        !/^[a-f0-9]{64}$/.test(artifact.sha256) ||
        typeof artifact.url !== "string"
      ) {
        issues.push(`${target}: invalid artifact record`);
        continue;
      }
      if (seenAssets.has(artifact.fileName)) {
        issues.push(
          `duplicate SourceForge filename across targets: ${artifact.fileName}`,
        );
      }
      seenAssets.add(artifact.fileName);
      if (!validateArtifactUrl(artifact, report)) {
        issues.push(`${target}: invalid public URL for ${artifact.fileName}`);
      }
    }
  }
  for (const target of expectedTargets) {
    if (!seenTargets.has(target)) {
      issues.push(`${target}: upload report is missing`);
    }
  }
  return issues;
}

export async function collectSourceforgeReports(
  root: string,
): Promise<unknown[]> {
  const reports: unknown[] = [];
  for await (const entry of Deno.readDir(root)) {
    const file = path.join(root, entry.name);
    if (entry.isDirectory) {
      reports.push(...await collectSourceforgeReports(file));
    } else if (entry.isFile && entry.name === "sourceforge-report.json") {
      reports.push(JSON.parse(await Deno.readTextFile(file)));
    }
  }
  return reports;
}

export interface PublicVerificationOptions {
  fetcher?: typeof fetch;
  wait?: (milliseconds: number) => Promise<void>;
  attempts?: number;
  timeoutMs?: number;
}

/** Stream through the hash without buffering or copying binaries to disk. */
export async function verifyPublicSourceforgeArtifact(
  artifact: SourceforgeArtifact,
  options: PublicVerificationOptions = {},
): Promise<void> {
  const fetcher = options.fetcher ?? fetch;
  const wait = options.wait ??
    ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
  const attempts = options.attempts ?? 8;
  if (!Number.isInteger(attempts) || attempts < 1 || attempts > 10) {
    throw new Error("verification attempts must be between 1 and 10");
  }
  let lastError: unknown;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    let response: Response | undefined;
    try {
      response = await fetcher(artifact.url, {
        redirect: "follow",
        signal: AbortSignal.timeout(options.timeoutMs ?? 900_000),
      });
      if (!response.ok || !response.body) {
        throw new Error(`public download returned HTTP ${response.status}`);
      }
      if (
        response.headers.get("content-type")?.toLowerCase().includes(
          "text/html",
        )
      ) throw new Error("public download returned HTML");
      const hash = createHash("sha256");
      let size = 0;
      const reader = response.body.getReader();
      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          size += value.byteLength;
          if (size > artifact.fileSize) {
            throw new Error("public download exceeds expected size");
          }
          hash.update(value);
        }
      } finally {
        await reader.cancel().catch(() => {});
        reader.releaseLock();
      }
      if (
        size !== artifact.fileSize || hash.digest("hex") !== artifact.sha256
      ) {
        throw new Error(
          "public size or SHA-256 does not match the build inventory",
        );
      }
      return;
    } catch (error) {
      lastError = error;
    } finally {
      if (response?.body && !response.body.locked) {
        await response.body.cancel().catch(() => {});
      }
    }
    if (attempt < attempts) await wait(Math.min(60_000, attempt * 10_000));
  }
  throw new Error(
    `${artifact.fileName}: verification failed after ${attempts} attempts`,
    { cause: lastError },
  );
}

export async function verifySourceforgeMirror(
  values: unknown[],
  expectedTargets: readonly string[],
  channel: "release" | "nightly",
  options: PublicVerificationOptions = {},
) {
  const issues = validateSourceforgeReports(values, expectedTargets);
  if (issues.length) throw new Error(issues.join("\n"));
  const reports = values as SourceforgeUploadReport[];
  const first = reports[0];
  if (first.channel !== channel) {
    throw new Error("report channel does not match the publishing job");
  }
  const assets: Record<
    string,
    { url: string; fileSize: number; sha256: string }
  > = {};
  for (const report of reports) {
    for (const artifact of report.artifacts) {
      await verifyPublicSourceforgeArtifact(artifact, options);
      assets[artifact.fileName] = {
        url: artifact.url,
        fileSize: artifact.fileSize,
        sha256: artifact.sha256,
      };
    }
  }
  return {
    schemaVersion: 1,
    project: first.project,
    channel,
    buildId: first.buildId,
    remotePath: first.remotePath,
    ...(first.publishedAt ? { publishedAt: first.publishedAt } : {}),
    assets,
  };
}

if (import.meta.main) {
  const [reportsPath, outputPath, channel, expectedTargetsArg] = Deno.args;
  if (
    !reportsPath || !outputPath || !["release", "nightly"].includes(channel) ||
    !expectedTargetsArg
  ) {
    throw new Error(
      "Usage: sourceforge:verify <reports-dir> <output.json> <release|nightly> <comma-separated-targets>",
    );
  }
  const result = await verifySourceforgeMirror(
    await collectSourceforgeReports(path.resolve(reportsPath)),
    expectedTargetsArg.split(",").filter(Boolean),
    channel as "release" | "nightly",
  );
  await Deno.writeTextFile(outputPath, `${JSON.stringify(result, null, 2)}\n`);
  const output = Deno.env.get("GITHUB_OUTPUT");
  if (output) {
    await Deno.writeTextFile(
      output,
      `sourceforge_mirrors_json=${
        JSON.stringify(result)
      }\nsourceforge_build_id=${result.buildId}\nsourceforge_project=${result.project}\n`,
      { append: true },
    );
  }
  console.log(
    `Publicly verified ${
      Object.keys(result.assets).length
    } SourceForge assets for ${result.remotePath}`,
  );
}
