import path from "node:path";
import fs from "fs-extra";
import { cwd, TAURI_APP_DIR } from "./utils/env";
import { consola } from "./utils/logger";

const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");
// TODO: define overrides
// const TAURI_DEV_APP_OVERRIDES_PATH = path.join(
//   TAURI_APP_DIR,
//   "overrides/nightly.conf.json",
// );
const PACKAGE_JSON_PATH = path.join(cwd, "package.json");
// blocked by https://github.com/tauri-apps/tauri/issues/8447
// const WXS_PATH = path.join(TAURI_APP_DIR, "templates", "nightly.wxs");

const isNSIS = process.argv.includes("--nsis"); // only build nsis

async function main() {
  consola.debug("Read config...");
  const tauriAppConf = await fs.readJSON(TAURI_APP_CONF);
  // const tauriAppOverrides = await fs.readJSON(TAURI_DEV_APP_OVERRIDES_PATH);
  // const tauriConf = merge(tauriAppConf, tauriAppOverrides);
  const tauriConf = tauriAppConf;
  // const wxsFile = await fs.readFile(WXS_PATH, "utf-8");

  if (isNSIS) {
    tauriConf.tauri.bundle.targets = ["nsis", "updater"];
  }

  consola.debug("Write tauri version to tauri.conf.json");
  await fs.writeJSON(TAURI_APP_CONF, tauriConf, { spaces: 2 });
  consola.debug("tauri.conf.json updated");

  consola.debug("package.json updated");
}

main();
