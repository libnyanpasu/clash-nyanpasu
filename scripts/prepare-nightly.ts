import { execSync } from "child_process";
import fs from "fs-extra";
import path from "node:path";
import { TAURI_APP_DIR, cwd } from "./utils/env";
import { consola } from "./utils/logger";

const TAURI_DEV_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "tauri.nightly.conf.json",
);
const PACKAGE_JSON_PATH = path.join(cwd, "package.json");

async function main() {
  const tauriConf = await fs.readJSON(TAURI_DEV_APP_CONF_PATH);
  const packageJson = await fs.readJSON(PACKAGE_JSON_PATH);
  consola.debug("Get current git short hash");
  const GIT_SHORT_HASH = execSync("git rev-parse --short HEAD")
    .toString()
    .trim();
  consola.debug(`Current git short hash: ${GIT_SHORT_HASH}`);

  const version = `${tauriConf.package.version}-alpha+${GIT_SHORT_HASH}`;
  // 1. update tauri version
  consola.debug("Write tauri version to tauri.nightly.conf.json");
  tauriConf.package.version = version;
  await fs.writeJSON(TAURI_DEV_APP_CONF_PATH, tauriConf, { spaces: 2 });
  consola.debug("tauri.nightly.conf.json updated");
  // 2. update package version
  consola.debug("Write tauri version to package.json");
  packageJson.version = version;
  await fs.writeJSON(PACKAGE_JSON_PATH, packageJson, { spaces: 2 });
  consola.debug("package.json updated");
}

main();
