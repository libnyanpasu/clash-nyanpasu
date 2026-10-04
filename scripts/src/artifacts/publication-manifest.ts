import * as path from "jsr:@std/path";
import { globby } from "npm:globby";
import { hashFile } from "./sourceforge.ts";

export type PublishChannel = "release" | "nightly";

export interface PublicationManifestInput {
  channel: PublishChannel;
  target: string;
  buildId: string;
  itemPrefix?: string;
  commit: string;
  tag: string | null;
  folderPath: string;
  itemIdentifier: string;
  publishedAt?: string;
  paths: string[];
}

export async function preparePublicationManifest(
  input: PublicationManifestInput,
  cwd = Deno.cwd(),
) {
  if (input.channel !== "release" && input.channel !== "nightly") {
    throw new Error("Invalid publication channel");
  }
  if (!/^[a-z0-9][a-z0-9_-]*$/.test(input.target)) {
    throw new Error(`Invalid target: ${input.target}`);
  }
  const buildIdentity = /^([0-9]+-[0-9]+-[a-f0-9]{40})-([a-z0-9][a-z0-9_-]*)$/
    .exec(input.buildId);
  if (!buildIdentity) {
    throw new Error(
      "Build id must include run, attempt, full commit and target",
    );
  }
  if (!/^[a-f0-9]{40}$/.test(input.commit)) {
    throw new Error("Commit must be a full Git SHA");
  }
  if (!/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(input.buildId)) {
    throw new Error("Invalid publication build id");
  }
  if (
    input.channel === "release" &&
    (input.folderPath !== `release/${input.tag}` || !input.tag ||
      !/^release\/[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(input.folderPath))
  ) throw new Error("Release folderPath must be release/<tag>");
  if (
    input.channel === "nightly" &&
    input.folderPath !== `nightly/${buildIdentity[1]}`
  ) throw new Error("Nightly folderPath must identify the global build");
  if (input.itemIdentifier.length > 100) {
    throw new Error("IA itemIdentifier exceeds 100 characters");
  }
  if (input.channel === "release" && !input.tag) {
    throw new Error("A release tag is required");
  }
  if (input.channel === "nightly" && input.tag !== null) {
    throw new Error("Nightly tag must be null");
  }
  if (input.paths.length === 0) {
    throw new Error("At least one artifact pattern is required");
  }

  const files = (await globby(input.paths, { cwd, absolute: true })).sort();
  if (files.length === 0) {
    throw new Error(`No files matched: ${input.paths.join(", ")}`);
  }
  const names = new Set<string>();
  const artifacts = [];
  for (const filePath of files) {
    const fileName = path.basename(filePath);
    if (
      !fileName || fileName === "." || fileName === ".." ||
      fileName.includes("/")
    ) throw new Error(`Invalid artifact filename: ${fileName}`);
    if (names.has(fileName)) {
      throw new Error(`Duplicate artifact filename: ${fileName}`);
    }
    names.add(fileName);
    const stat = await Deno.stat(filePath);
    if (!stat.isFile || stat.size <= 0) {
      throw new Error(`Artifact is empty or not a file: ${fileName}`);
    }
    const digest = await hashFile(filePath);
    if (digest.fileSize !== stat.size) {
      throw new Error(`Artifact changed while hashing: ${fileName}`);
    }
    artifacts.push({
      fileName,
      fileSize: stat.size,
      sha256: digest.sha256,
      md5: digest.md5,
      path: filePath,
    });
  }

  return {
    schemaVersion: 1 as const,
    buildId: input.buildId,
    itemIdentifier: input.itemIdentifier,
    channel: input.channel,
    commit: input.commit,
    tag: input.tag,
    folderPath: input.folderPath,
    ...(input.publishedAt === undefined
      ? {}
      : { publishedAt: input.publishedAt }),
    target: input.target,
    artifacts,
  };
}

export function createIaItemIdentifier(
  prefix: string,
  channel: PublishChannel,
  runId: string,
  attempt: string,
  target: string,
  commit: string,
): string {
  if (!/^[A-Za-z0-9_-]+$/.test(prefix)) {
    throw new Error("IA item prefix contains unsupported characters");
  }
  if (!/^[0-9]+$/.test(runId) || !/^[0-9]+$/.test(attempt)) {
    throw new Error("Invalid workflow run identity");
  }
  if (!/^[a-z0-9][a-z0-9_-]*$/.test(target) || !/^[a-f0-9]{40}$/.test(commit)) {
    throw new Error("Invalid target or commit identity");
  }
  const identifier = `${prefix}-${channel}-${runId}-${attempt}-${target}-${
    commit.slice(0, 12)
  }`;
  if (identifier.length > 100) {
    throw new Error(
      `IA itemIdentifier exceeds 100 characters (${identifier.length})`,
    );
  }
  return identifier;
}
