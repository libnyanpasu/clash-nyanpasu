import { parseArgs } from "jsr:@std/cli@1/parse-args";
import * as path from "jsr:@std/path";
import {
  CENTRAL_PUBLICATION_TARGETS,
  prepareCentralPublication,
} from "./central-publication.ts";

interface SourceRun {
  id: number;
  run_attempt: number;
  head_sha: string;
  status: string;
  event: string;
  path: string;
  repository: { full_name: string };
  head_repository: { full_name: string };
}

export async function recoverCentralPublication(
  run: SourceRun,
  repository: string,
  artifactsRoot: string,
  previousRoot: string,
  outputRoot: string,
  itemPrefix: string,
) {
  if (
    run.status !== "completed" || run.repository.full_name !== repository ||
    run.head_repository.full_name !== repository ||
    !["workflow_dispatch", "schedule", "release"].includes(run.event) ||
    ![
      ".github/workflows/target-dev-build.yaml",
      ".github/workflows/target-release-build.yaml",
    ].includes(run.path)
  ) {
    throw new Error(
      "Source run must be a completed package publication in this repository",
    );
  }
  const previous = JSON.parse(
    await Deno.readTextFile(
      path.join(previousRoot, "publication-context.json"),
    ),
  );
  const nightly = run.path.endsWith("target-dev-build.yaml");
  if (
    previous.commit !== run.head_sha ||
    previous.channel !== (nightly ? "nightly" : "release")
  ) {
    throw new Error("Saved publication context does not match the source run");
  }
  const saved = await Promise.all(
    CENTRAL_PUBLICATION_TARGETS.map(async (target) =>
      JSON.parse(
        await Deno.readTextFile(
          path.join(previousRoot, "manifests", `${target}.json`),
        ),
      )
    ),
  );
  const identity = /^(\d+)-(\d+)-([a-f0-9]{40})-windows-x86_64$/.exec(
    saved[0].buildId,
  );
  if (
    !identity || identity[1] !== String(run.id) ||
    identity[3] !== run.head_sha ||
    Number(identity[2]) > run.run_attempt
  ) throw new Error("Saved manifests do not match the source run identity");
  const context = await prepareCentralPublication(artifactsRoot, outputRoot, {
    channel: previous.channel,
    tag: previous.tag,
    commit: run.head_sha,
    runId: identity[1],
    attempt: identity[2],
    itemPrefix,
    publishedAt: previous.publishedAt,
  });
  // Paths move between runners; all published metadata and bytes must stay identical.
  for (const [index, target] of CENTRAL_PUBLICATION_TARGETS.entries()) {
    const current = JSON.parse(
      await Deno.readTextFile(context.manifests[target] as string),
    );
    const immutable = (manifest: typeof current) => ({
      schemaVersion: manifest.schemaVersion,
      buildId: manifest.buildId,
      itemIdentifier: manifest.itemIdentifier,
      channel: manifest.channel,
      commit: manifest.commit,
      tag: manifest.tag,
      target: manifest.target,
      folderPath: manifest.folderPath,
      publishedAt: manifest.publishedAt,
      artifacts: manifest.artifacts.map((
        artifact: {
          fileName: string;
          fileSize: number;
          sha256: string;
          md5: string;
        },
      ) => ({
        fileName: artifact.fileName,
        fileSize: artifact.fileSize,
        sha256: artifact.sha256,
        md5: artifact.md5,
      })).sort((a: { fileName: string }, b: { fileName: string }) =>
        a.fileName.localeCompare(b.fileName)
      ),
    });
    if (
      JSON.stringify(immutable(current)) !==
        JSON.stringify(immutable(saved[index]))
    ) {
      throw new Error(
        `Recovered inventory differs from the original publication for ${target}`,
      );
    }
  }
  if (
    context.buildId !== previous.buildId ||
    context.folderPath !== previous.folderPath
  ) {
    throw new Error(
      "Recovered global publication identity differs from the original",
    );
  }
  return context;
}

if (import.meta.main) {
  const args = parseArgs(Deno.args, {
    string: [
      "source-run",
      "artifacts-dir",
      "previous-dir",
      "output-dir",
      "item-prefix",
    ],
  });
  for (
    const name of [
      "source-run",
      "artifacts-dir",
      "previous-dir",
      "output-dir",
      "item-prefix",
    ]
  ) {
    if (!args[name]) throw new Error(`--${name} is required`);
  }
  const context = await recoverCentralPublication(
    JSON.parse(await Deno.readTextFile(args["source-run"]!)),
    Deno.env.get("GITHUB_REPOSITORY")!,
    args["artifacts-dir"]!,
    args["previous-dir"]!,
    args["output-dir"]!,
    args["item-prefix"]!,
  );
  const outputs =
    `channel=${context.channel}\nbuild_id=${context.buildId}\ntag=${
      context.tag ?? ""
    }\n`;
  if (Deno.env.get("GITHUB_OUTPUT")) {
    await Deno.writeTextFile(Deno.env.get("GITHUB_OUTPUT")!, outputs, {
      append: true,
    });
  }
  console.log(
    `Recovered ${context.buildId} at original timestamp ${context.publishedAt}`,
  );
}
