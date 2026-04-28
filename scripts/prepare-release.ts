import * as path from "jsr:@std/path";
import { merge } from "npm:lodash-es";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_FIXED_WEBVIEW2_CONFIG_OVERRIDE_PATH = path.join(
  TAURI_APP_DIR,
  "overrides/fixed-webview2.conf.json",
);
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");

const fixedWebview = Deno.args.includes("--fixed-webview");

async function main() {
  consola.debug("Read config...");
  let tauriConf = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF));

  if (fixedWebview) {
    const fixedWebview2Config = JSON.parse(
      await Deno.readTextFile(TAURI_FIXED_WEBVIEW2_CONFIG_OVERRIDE_PATH),
    );
    let webviewPath: string | undefined;
    for await (const entry of Deno.readDir(TAURI_APP_DIR)) {
      if (entry.name.includes("WebView2")) {
        webviewPath = entry.name;
        break;
      }
    }
    if (!webviewPath) {
      throw new Error("WebView2 runtime not found");
    }
    tauriConf = merge(tauriConf, fixedWebview2Config);
    delete tauriConf.bundle.windows.webviewInstallMode.silent;
    tauriConf.bundle.windows.webviewInstallMode.path = `./${
      path.basename(webviewPath)
    }`;
  }

  consola.debug("Write tauri version to tauri.conf.json");
  await Deno.writeTextFile(TAURI_APP_CONF, JSON.stringify(tauriConf, null, 2));
  consola.debug("tauri.conf.json updated");
}

main();
