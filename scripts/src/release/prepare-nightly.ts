import * as path from "node:path";
import { merge } from "es-toolkit";
import { consola } from "../shared/logger.ts";
import { readFile, writeFile } from "node:fs/promises";
import { runCommand } from "../shared/command.ts";

const cwd = process.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
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

const isNSIS = process.argv.slice(2).includes("--nsis");
const isMSI = process.argv.slice(2).includes("--msi");
const isLinux = process.argv.slice(2).includes("--linux");
const disableUpdater = process.argv.slice(2).includes("--disable-updater");

async function main() {
  consola.debug("Read config...");
  const tauriAppConf = JSON.parse(await readFile(TAURI_APP_CONF, "utf8"));
  const tauriAppOverrides = JSON.parse(
    await readFile(TAURI_DEV_APP_OVERRIDES_PATH, "utf8"),
  );
  const tauriConf = merge(tauriAppConf, tauriAppOverrides);
  const packageJson = JSON.parse(
    await readFile(NYANPASU_PACKAGE_JSON_PATH, "utf8"),
  );
  const rootPackageJson = JSON.parse(
    await readFile(ROOT_PACKAGE_JSON_PATH, "utf8"),
  );

  if (isNSIS) {
    tauriConf.bundle.targets = ["nsis"];
  }

  if (disableUpdater) {
    tauriConf.bundle.createUpdaterArtifacts = false;
  }

  consola.debug("Get current git short hash");
  const gitResult = await runCommand("git", {
    args: ["rev-parse", "HEAD"],
    stdout: "piped",
  });
  // `--short` scales with the clone's object count, so shallow build jobs and
  // the full-history updater job disagree; the signed version must match both.
  const GIT_SHORT_HASH = gitResult.stdout.toString("utf8")
    .trim()
    .slice(0, 7);
  consola.debug(`Current git short hash: ${GIT_SHORT_HASH}`);

  const version = `${tauriConf.version}-alpha+${GIT_SHORT_HASH}`;
  // RPM/DEB forbid `-` in the version string (#3850), but tauri requires
  // `version` to be valid semver, so `~` is not usable here. `+` is legal in
  // semver (build metadata) and in RPM/DEB version strings, so use it instead.
  const linuxVersion = `${tauriConf.version}+alpha.${GIT_SHORT_HASH}`;

  consola.debug("Write tauri version to tauri.nightly.conf.json");
  if (isLinux) {
    tauriConf.version = linuxVersion;
  } else if (!isNSIS && !isMSI) {
    tauriConf.version = version;
  }
  await writeFile(TAURI_DEV_APP_CONF_PATH, JSON.stringify(tauriConf, null, 2));
  consola.debug("tauri.nightly.conf.json updated");

  consola.debug("Write tauri version to package.json");
  packageJson.version = version;
  await writeFile(
    NYANPASU_PACKAGE_JSON_PATH,
    JSON.stringify(packageJson, null, 2),
  );
  rootPackageJson.version = version;
  await writeFile(
    ROOT_PACKAGE_JSON_PATH,
    JSON.stringify(rootPackageJson, null, 2),
  );
  consola.debug("package.json updated");
}

main();
