import * as path from "node:path";
import semver from "npm:semver";
import { readdir, readFile, writeFile } from "node:fs/promises";

const cwd = process.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF_PATH = path.join(TAURI_APP_DIR, "tauri.conf.json");
const TAURI_NIGHTLY_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "overrides/nightly.conf.json",
);
const PACKAGE_JSON_PATH = path.join(cwd, "package.json");

export const RELEASE_TYPES = [
  "major",
  "minor",
  "patch",
  "premajor",
  "preminor",
  "prepatch",
  "prerelease",
] as const;
export type ReleaseType = typeof RELEASE_TYPES[number];

export const PRERELEASE_IDS = ["beta", "rc"] as const;
export type PrereleaseId = typeof PRERELEASE_IDS[number];

export function resolveNextVersions(
  current: string,
  releaseType: ReleaseType,
  preid: PrereleaseId,
) {
  if (!RELEASE_TYPES.includes(releaseType)) {
    throw new Error(`invalid release type "${releaseType}"`);
  }
  if (!PRERELEASE_IDS.includes(preid)) {
    throw new Error(`invalid prerelease id "${preid}"`);
  }
  // Prerelease numbers start at 1 (2.1.0-beta.1); the beta updater channel
  // takes every version with a prerelease component.
  const version = semver.inc(current, releaseType, preid, "1");
  if (!version) throw new Error(`invalid current version "${current}"`);
  // e.g. `prerelease beta` from 2.0.0-rc.1 would go back to 2.0.0-beta.1.
  if (!semver.gt(version, current)) {
    throw new Error(`${releaseType} ${preid} does not advance ${current}`);
  }
  const { major, minor, patch } = semver.parse(version)!;
  // Nightly builds sort after the release they follow, including its betas.
  return { version, nightlyVersion: `${major}.${minor}.${patch + 1}` };
}

async function resolvePublish() {
  const releaseType = (process.argv.slice(2)[0] ?? "patch") as ReleaseType;
  const preid = (process.argv.slice(2)[1] ?? "beta") as PrereleaseId;
  const packageJson = JSON.parse(await readFile(PACKAGE_JSON_PATH, "utf8"));
  const tauriJson = JSON.parse(await readFile(TAURI_APP_CONF_PATH, "utf8"));
  const tauriNightlyJson = JSON.parse(
    await readFile(TAURI_NIGHTLY_APP_CONF_PATH, "utf8"),
  );

  const { version: nextVersion, nightlyVersion: nextNightlyVersion } =
    resolveNextVersions(packageJson.version, releaseType, preid);

  packageJson.version = nextVersion;
  tauriJson.version = nextVersion;
  tauriNightlyJson.version = nextNightlyVersion;

  await writeFile(PACKAGE_JSON_PATH, JSON.stringify(packageJson, null, 2));
  await writeFile(TAURI_APP_CONF_PATH, JSON.stringify(tauriJson, null, 2));
  await writeFile(
    TAURI_NIGHTLY_APP_CONF_PATH,
    JSON.stringify(tauriNightlyJson, null, 2),
  );

  for (
    const entry of await readdir(path.join(cwd, "frontend"), {
      withFileTypes: true,
    })
  ) {
    if (!entry.isDirectory()) continue;
    const monoRepoPackageJsonPath = path.join(
      cwd,
      "frontend",
      entry.name,
      "package.json",
    );
    try {
      const monoRepoPackageJson = JSON.parse(
        await readFile(monoRepoPackageJsonPath, "utf8"),
      );
      monoRepoPackageJson.version = nextVersion;
      await writeFile(
        monoRepoPackageJsonPath,
        JSON.stringify(monoRepoPackageJson, null, 2),
      );
    } catch {
      // package may not exist
    }
  }

  console.log(nextVersion);
}

if (import.meta.main) {
  resolvePublish();
}
