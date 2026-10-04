import * as path from "jsr:@std/path";

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validPublicationTime(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    return new Date(value).toISOString() === value;
  } catch {
    return false;
  }
}

/** Pending ingest is acceptable only after a complete upload; missing bytes are errors. */
export function validateIaReports(
  reports: unknown[],
  expectedTargets: readonly string[],
): { issues: string[]; pending: string[] } {
  const issues: string[] = [];
  const pending: string[] = [];
  const targets = new Set<string>();
  const items = new Set<string>();
  let identity: string | undefined;
  if (
    !expectedTargets.length ||
    new Set(expectedTargets).size !== expectedTargets.length
  ) issues.push("Expected targets must be non-empty and unique");
  for (const value of reports) {
    if (!record(value)) {
      issues.push("IA report must be an object");
      continue;
    }
    const target = typeof value.target === "string"
      ? value.target
      : "(missing)";
    if (!expectedTargets.includes(target) || targets.has(target)) {
      issues.push(`Unexpected or duplicate IA target: ${target}`);
    }
    targets.add(target);
    if (
      value.schemaVersion !== 1 ||
      !["pending", "ready"].includes(String(value.status))
    ) {
      issues.push(
        `${target}: IA upload failed: ${String(value.error ?? value.status)}`,
      );
      continue;
    }
    if (
      typeof value.buildId !== "string" ||
      !value.buildId.endsWith(`-${target}`) ||
      typeof value.itemIdentifier !== "string" ||
      !/^[A-Za-z0-9_-]{1,100}$/.test(value.itemIdentifier) ||
      typeof value.commit !== "string" ||
      !/^[a-f0-9]{40}$/.test(value.commit) ||
      !["nightly", "release"].includes(String(value.channel)) ||
      typeof value.folderPath !== "string" ||
      !value.folderPath.startsWith(`${value.channel}/`) ||
      !validPublicationTime(value.publishedAt)
    ) {
      issues.push(`${target}: Invalid IA build identity`);
      continue;
    }
    if (items.has(value.itemIdentifier)) {
      issues.push(`${target}: IA item reused across targets`);
    }
    items.add(value.itemIdentifier);
    const currentIdentity = JSON.stringify([
      value.buildId.slice(0, -(target.length + 1)),
      value.commit,
      value.channel,
      value.folderPath,
      value.publishedAt,
    ]);
    identity ??= currentIdentity;
    if (identity !== currentIdentity) {
      issues.push(`${target}: IA reports belong to different builds`);
    }
    if (
      !Array.isArray(value.artifacts) || !value.artifacts.length ||
      !Array.isArray(value.uploads)
    ) {
      issues.push(
        `${target}: Missing registered artifacts or upload inventory`,
      );
      continue;
    }
    const names = new Set<string>();
    for (const artifact of value.artifacts) {
      if (
        !record(artifact) || typeof artifact.fileName !== "string" ||
        !artifact.fileName ||
        /[\\/\r\n\0]/.test(artifact.fileName) ||
        artifact.fileName === "artifact-manifest.json" ||
        names.has(artifact.fileName) || typeof artifact.fileId !== "string" ||
        !Number.isSafeInteger(artifact.fileSize) ||
        Number(artifact.fileSize) <= 0 ||
        typeof artifact.sha256 !== "string" ||
        !/^[a-f0-9]{64}$/.test(artifact.sha256) ||
        typeof artifact.md5 !== "string" ||
        !/^[a-f0-9]{32}$/.test(artifact.md5) ||
        artifact.storageKey !== artifact.fileName ||
        artifact.status !== value.status
      ) {
        issues.push(`${target}: Invalid registered artifact`);
        continue;
      }
      names.add(artifact.fileName);
    }
    const uploaded = new Set<string>();
    for (const upload of value.uploads) {
      if (
        !record(upload) || typeof upload.fileName !== "string" ||
        uploaded.has(upload.fileName) ||
        !["uploaded", "skipped"].includes(String(upload.status)) ||
        (!names.has(upload.fileName) &&
          upload.fileName !== "artifact-manifest.json")
      ) {
        issues.push(`${target}: Invalid upload inventory`);
        continue;
      }
      uploaded.add(upload.fileName);
    }
    if (
      uploaded.size !== names.size + 1 ||
      !uploaded.has("artifact-manifest.json") || [...names].some((name) =>
        !uploaded.has(name)
      )
    ) issues.push(`${target}: Registered artifacts were not all uploaded`);
    if (value.status === "pending") {
      pending.push(`${target}: ${value.buildId} / ${value.itemIdentifier}`);
    }
  }
  for (const target of expectedTargets) {
    if (!targets.has(target)) issues.push(`${target}: IA report is missing`);
  }
  return { issues, pending };
}

export async function collectIaReports(root: string): Promise<unknown[]> {
  const reports: unknown[] = [];
  for await (const entry of Deno.readDir(root)) {
    const file = path.join(root, entry.name);
    if (entry.isDirectory) reports.push(...await collectIaReports(file));
    else if (entry.isFile && entry.name === "ia-report.json") {
      reports.push(JSON.parse(await Deno.readTextFile(file)));
    }
  }
  return reports;
}

if (import.meta.main) {
  const [root, targetList] = Deno.args;
  if (!root || !targetList) {
    throw new Error(
      "Usage: archive:verify-reports <reports-dir> <comma-separated-targets>",
    );
  }
  const result = validateIaReports(
    await collectIaReports(root),
    targetList.split(","),
  );
  const summary = result.issues.length
    ? `IA archive incomplete:\n${result.issues.join("\n")}`
    : result.pending.length
    ? `IA bytes uploaded; ingest pending (rerun archive verification):\n${
      result.pending.join("\n")
    }`
    : "All target IA archives are ready";
  console.log(summary);
  const summaryPath = Deno.env.get("GITHUB_STEP_SUMMARY");
  if (summaryPath) {
    await Deno.writeTextFile(summaryPath, `\n${summary}\n`, { append: true });
  }
  if (result.issues.length) {
    throw new Error("Incomplete or failed IA archive uploads");
  }
}
