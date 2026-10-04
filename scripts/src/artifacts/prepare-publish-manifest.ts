import { parseArgs } from "jsr:@std/cli@1/parse-args";
import {
  createIaItemIdentifier,
  preparePublicationManifest,
} from "./publication-manifest.ts";

async function main(): Promise<void> {
  const args = parseArgs(Deno.args, {
    string: [
      "channel",
      "target",
      "build-id",
      "item-prefix",
      "commit",
      "tag",
      "folder",
      "output",
    ],
    collect: ["pattern"],
    alias: { pattern: "p" },
  });
  const requireValue = (value: string | undefined, name: string): string => {
    if (!value) throw new Error(`--${name} is required`);
    return value;
  };
  const channel = requireValue(args.channel, "channel") as
    | "release"
    | "nightly";
  const target = requireValue(args.target, "target");
  const commit = requireValue(args.commit, "commit");
  const itemPrefix = args["item-prefix"] || "sourceforge-only";
  const runId = Deno.env.get("GITHUB_RUN_ID") ?? "0";
  const attempt = Deno.env.get("GITHUB_RUN_ATTEMPT") ?? "1";
  const itemIdentifier = createIaItemIdentifier(
    itemPrefix,
    channel,
    runId,
    attempt,
    target,
    commit,
  );
  const manifest = await preparePublicationManifest({
    channel,
    target,
    buildId: requireValue(args["build-id"], "build-id"),
    itemPrefix,
    commit,
    tag: channel === "release" ? requireValue(args.tag, "tag") : null,
    folderPath: requireValue(args.folder, "folder"),
    itemIdentifier,
    paths: ((args.pattern ?? []) as string[]).filter(Boolean),
  });
  const output = requireValue(args.output, "output");
  await Deno.writeTextFile(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(
    `Prepared ${manifest.artifacts.length} artifacts for ${target}: ${output}`,
  );
}

if (import.meta.main) await main();
