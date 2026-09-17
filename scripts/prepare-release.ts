import * as path from "jsr:@std/path";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");

async function main() {
  consola.debug("Read config...");
  const tauriConf = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF));

  consola.debug("Write release config to tauri.conf.json");
  await Deno.writeTextFile(TAURI_APP_CONF, JSON.stringify(tauriConf, null, 2));
  consola.debug("tauri.conf.json updated");
}

main();
