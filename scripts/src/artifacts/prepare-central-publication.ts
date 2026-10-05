import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { prepareCentralPublication } from "./central-publication.ts";

async function main(): Promise<void> {
  const args = parseArgs(Deno.args, {
    string: [
      "channel",
      "artifacts-dir",
      "output-dir",
      "commit",
      "run-id",
      "attempt",
      "tag",
      "item-prefix",
      "published-at",
    ],
  });
  const required = (value: string | undefined, name: string): string => {
    if (!value) throw new Error(`--${name} is required`);
    return value;
  };
  const channel = required(args.channel, "channel");
  if (channel !== "nightly" && channel !== "release") {
    throw new Error("--channel must be nightly or release");
  }
  const context = await prepareCentralPublication(
    required(args["artifacts-dir"], "artifacts-dir"),
    required(args["output-dir"], "output-dir"),
    {
      channel,
      tag: channel === "release" ? required(args.tag, "tag") : null,
      commit: required(args.commit, "commit"),
      runId: required(args["run-id"], "run-id"),
      attempt: required(args.attempt, "attempt"),
      itemPrefix: args["item-prefix"] ?? "sourceforge-only",
      publishedAt: args["published-at"],
    },
  );
  if (Deno.env.get("GITHUB_OUTPUT")) {
    await Deno.writeTextFile(
      Deno.env.get("GITHUB_OUTPUT")!,
      `published_at=${context.publishedAt}\n`,
      { append: true },
    );
  }
  console.log(
    `Prepared ${context.targets.length} target inventories at ${context.publishedAt}`,
  );
}

if (import.meta.main) await main();
