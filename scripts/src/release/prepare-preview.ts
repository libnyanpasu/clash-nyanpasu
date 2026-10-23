import * as path from "node:path";
import { consola } from "../shared/logger.ts";
import { readFile, writeFile } from "node:fs/promises";

const cwd = process.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");
const TAURI_PREVIEW_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "tauri.preview.conf.json",
);

const main = async () => {
  consola.debug("Read config...");
  const tauriAppConf = JSON.parse(await readFile(TAURI_APP_CONF, "utf8"));
  tauriAppConf.build.devPath = tauriAppConf.build.distDir;
  tauriAppConf.build.beforeDevCommand = tauriAppConf.build.beforeBuildCommand;
  consola.debug("Write config...");
  await writeFile(
    TAURI_PREVIEW_APP_CONF_PATH,
    JSON.stringify(tauriAppConf, null, 2),
  );
};

main();
