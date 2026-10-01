import * as path from "jsr:@std/path";
import semver from "npm:semver";
import { consola } from "./utils/logger.ts";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, "tauri.conf.json");

const isLinux = Deno.args.includes("--linux");

/**
 * System package versions for a release. RPM forbids `-` in Version (#3850)
 * and both dpkg and rpm sort `2.0.0-beta.1` or `2.0.0+beta.1` after `2.0.0`.
 * Tauri requires a semver `version` shared by every bundle, so Linux bundles
 * `X.Y.Z`, rpm marks the prerelease in Release (`0.beta.1` sorts before the
 * default `1`), and finalize-linux-release.ts rewrites the deb Version to
 * `X.Y.Z~beta.1`. The app itself still reports package.json's version.
 */
export function linuxPackageVersions(version: string) {
  const parsed = semver.parse(version);
  if (!parsed) throw new Error(`invalid version "${version}"`);
  if (parsed.build.length > 0) {
    throw new Error(`release version "${version}" has build metadata`);
  }
  const tauriVersion = `${parsed.major}.${parsed.minor}.${parsed.patch}`;
  if (parsed.prerelease.length === 0) {
    return { tauriVersion, rpmRelease: "1", debVersion: tauriVersion };
  }
  const prerelease = parsed.prerelease.join(".");
  return {
    tauriVersion,
    rpmRelease: `0.${prerelease}`,
    debVersion: `${tauriVersion}~${prerelease}`,
  };
}

async function main() {
  consola.debug("Read config...");
  const tauriConf = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF));

  if (isLinux) {
    const { tauriVersion, rpmRelease } = linuxPackageVersions(
      tauriConf.version,
    );
    tauriConf.version = tauriVersion;
    tauriConf.bundle.linux.rpm = {
      ...tauriConf.bundle.linux.rpm,
      release: rpmRelease,
    };
  }

  consola.debug("Write release config to tauri.conf.json");
  await Deno.writeTextFile(TAURI_APP_CONF, JSON.stringify(tauriConf, null, 2));
  consola.debug("tauri.conf.json updated");
}

if (import.meta.main) {
  main();
}
