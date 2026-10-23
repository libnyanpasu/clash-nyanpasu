import * as path from "node:path";
import { runCommand } from "../shared/command.ts";
import { consola } from "../shared/logger.ts";
import { mkdir, writeFile } from "node:fs/promises";

const cwd = process.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_TEMP_DIR = path.join(TAURI_APP_DIR, "tmp");
const GIT_SUMMARY_INFO_PATH = path.join(TAURI_APP_TEMP_DIR, "git-info.json");

async function main() {
  const result = await runCommand("git", {
    args: [
      "show",
      "--pretty=format:'%H,%an,%cI'",
      "--no-patch",
      "--no-notes",
    ],
    stdout: "piped",
  });

  const output = result.stdout.toString("utf8")
    .replace(/'/g, "")
    .trim();
  const [hash, author, time] = output.split(",");

  const summary = { hash, author, time };
  consola.info(summary);

  await mkdir(TAURI_APP_TEMP_DIR, { recursive: true });
  await writeFile(GIT_SUMMARY_INFO_PATH, JSON.stringify(summary, null, 2));
  consola.success("Git summary info generated");
}

main().catch(consola.error);
