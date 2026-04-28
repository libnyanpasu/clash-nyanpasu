import * as path from "jsr:@std/path";

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
  path.join(cwd, "frontend/ui"),
  path.join(cwd, "frontend/interface"),
];

async function resolvePublish() {
  const flag = Deno.args[0] ?? "patch";
  const packageJson = JSON.parse(await Deno.readTextFile(PACKAGE_JSON_PATH));
  const tauriJson = JSON.parse(await Deno.readTextFile(TAURI_APP_CONF_PATH));
  const tauriNightlyJson = JSON.parse(
    await Deno.readTextFile(TAURI_NIGHTLY_APP_CONF_PATH),
  );

  let [a, b, c] = packageJson.version.split(".").map(Number);

  if (flag === "major") {
    a += 1;
    b = 0;
    c = 0;
  } else if (flag === "minor") {
    b += 1;
    c = 0;
  } else if (flag === "patch") {
    c += 1;
  } else {
    throw new Error(`invalid flag "${flag}"`);
  }

  const nextVersion = `${a}.${b}.${c}`;
  const nextNightlyVersion = `${a}.${b}.${c + 1}`;

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
      // package may not exist (e.g., frontend/ui)
    }
  }

  console.log(nextVersion);
}

resolvePublish();
