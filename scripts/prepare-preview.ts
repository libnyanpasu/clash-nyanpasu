import * as path from "jsr:@std/path";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");
const TAURI_PREVIEW_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "tauri.preview.conf.json",
);

const main = async () => {
  consola.debug("Read config...");
  const tauriAppConf = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF));
  tauriAppConf.build.devPath = tauriAppConf.build.distDir;
  tauriAppConf.build.beforeDevCommand = tauriAppConf.build.beforeBuildCommand;
  consola.debug("Write config...");
  await Deno.writeTextFile(
    TAURI_PREVIEW_APP_CONF_PATH,
    JSON.stringify(tauriAppConf, null, 2),
  );
};

main();
