import * as path from "jsr:@std/path";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_TEMP_DIR = path.join(TAURI_APP_DIR, "tmp");
const GIT_SUMMARY_INFO_PATH = path.join(TAURI_APP_TEMP_DIR, "git-info.json");

async function main() {
  const result = await new Deno.Command("git", {
    args: [
      "show",
      "--pretty=format:'%H,%cn,%cI'",
      "--no-patch",
      "--no-notes",
    ],
    stdout: "piped",
  }).output();

  const output = new TextDecoder()
    .decode(result.stdout)
    .replace(/'/g, "")
    .trim();
  const [hash, author, time] = output.split(",");

  const summary = { hash, author, time };
  consola.info(summary);

  await Deno.mkdir(TAURI_APP_TEMP_DIR, { recursive: true });
  await Deno.writeTextFile(
    GIT_SUMMARY_INFO_PATH,
    JSON.stringify(summary, null, 2),
  );
  consola.success("Git summary info generated");
}

main().catch(consola.error);
