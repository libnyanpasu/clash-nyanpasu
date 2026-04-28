import * as path from "jsr:@std/path";
import { merge } from "npm:lodash-es";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_FIXED_WEBVIEW2_CONFIG_OVERRIDE_PATH = path.join(
  TAURI_APP_DIR,
  "overrides/fixed-webview2.conf.json",
);
const TAURI_DEV_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "tauri.nightly.conf.json",
);
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");
const TAURI_DEV_APP_OVERRIDES_PATH = path.join(
  TAURI_APP_DIR,
  "overrides/nightly.conf.json",
);
const NYANPASU_PACKAGE_JSON_PATH = path.join(
  cwd,
  "frontend/nyanpasu/package.json",
);
const ROOT_PACKAGE_JSON_PATH = path.join(cwd, "package.json");

const isNSIS = Deno.args.includes("--nsis");
const isMSI = Deno.args.includes("--msi");
const fixedWebview = Deno.args.includes("--fixed-webview");
const disableUpdater = Deno.args.includes("--disable-updater");

async function main() {
  consola.debug("Read config...");
  const tauriAppConf = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF));
  const tauriAppOverrides = JSON.parse(
    await Deno.readTextFile(TAURI_DEV_APP_OVERRIDES_PATH),
  );
  let tauriConf = merge(tauriAppConf, tauriAppOverrides);
  const packageJson = JSON.parse(
    await Deno.readTextFile(NYANPASU_PACKAGE_JSON_PATH),
  );
  const rootPackageJson = JSON.parse(
    await Deno.readTextFile(ROOT_PACKAGE_JSON_PATH),
  );

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
    tauriConf.plugins.updater.endpoints = tauriConf.plugins.updater.endpoints
      .map((o: string) => o.replace("update-", "update-nightly-"));
  }

  if (isNSIS) {
    tauriConf.bundle.targets = ["nsis"];
  }

  if (disableUpdater) {
    tauriConf.bundle.createUpdaterArtifacts = false;
  }

  consola.debug("Get current git short hash");
  const gitResult = await new Deno.Command("git", {
    args: ["rev-parse", "--short", "HEAD"],
    stdout: "piped",
  }).output();
  const GIT_SHORT_HASH = new TextDecoder().decode(gitResult.stdout).trim();
  consola.debug(`Current git short hash: ${GIT_SHORT_HASH}`);

  const version = `${tauriConf.version}-alpha+${GIT_SHORT_HASH}`;

  consola.debug("Write tauri version to tauri.nightly.conf.json");
  if (!isNSIS && !isMSI) tauriConf.version = version;
  await Deno.writeTextFile(
    TAURI_DEV_APP_CONF_PATH,
    JSON.stringify(tauriConf, null, 2),
  );
  consola.debug("tauri.nightly.conf.json updated");

  consola.debug("Write tauri version to package.json");
  packageJson.version = version;
  await Deno.writeTextFile(
    NYANPASU_PACKAGE_JSON_PATH,
    JSON.stringify(packageJson, null, 2),
  );
  rootPackageJson.version = version;
  await Deno.writeTextFile(
    ROOT_PACKAGE_JSON_PATH,
    JSON.stringify(rootPackageJson, null, 2),
  );
  consola.debug("package.json updated");
}

main();
