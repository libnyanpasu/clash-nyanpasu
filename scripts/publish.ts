import * as path from "jsr:@std/path";
import semver from "npm:semver";

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
const TAURI_APP_CONF_PATH = path.join(TAURI_APP_DIR, "tauri.conf.json");
const TAURI_NIGHTLY_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  "overrides/nightly.conf.json",
);
const PACKAGE_JSON_PATH = path.join(cwd, "package.json");

const MONO_REPO_PATHS = [
  path.join(cwd, "frontend/nyanpasu"),
  path.join(cwd, "frontend/utils"),
  path.join(cwd, "frontend/interface"),
];

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
  const releaseType = (Deno.args[0] ?? "patch") as ReleaseType;
  const preid = (Deno.args[1] ?? "beta") as PrereleaseId;
  const packageJson = JSON.parse(await Deno.readTextFile(PACKAGE_JSON_PATH));
  const tauriJson = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF_PATH));
  const tauriNightlyJson = JSON.parse(
    await Deno.readTextFile(TAURI_NIGHTLY_APP_CONF_PATH),
  );

  const { version: nextVersion, nightlyVersion: nextNightlyVersion } =
    resolveNextVersions(packageJson.version, releaseType, preid);

  packageJson.version = nextVersion;
  tauriJson.version = nextVersion;
  tauriNightlyJson.version = nextNightlyVersion;

  await Deno.writeTextFile(
    PACKAGE_JSON_PATH,
    JSON.stringify(packageJson, null, 2),
  );
  await Deno.writeTextFile(
    TAURI_APP_CONF_PATH,
    JSON.stringify(tauriJson, null, 2),
  );
  await Deno.writeTextFile(
    TAURI_NIGHTLY_APP_CONF_PATH,
    JSON.stringify(tauriNightlyJson, null, 2),
  );

  for (const monoRepoPath of MONO_REPO_PATHS) {
    const monoRepoPackageJsonPath = path.join(monoRepoPath, "package.json");
    try {
      const monoRepoPackageJson = JSON.parse(
        await Deno.readTextFile(monoRepoPackageJsonPath),
      );
      monoRepoPackageJson.version = nextVersion;
      await Deno.writeTextFile(
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
