import {
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import * as path from "node:path";
import { parseArgs } from "node:util";
import { runCommand } from "../../shared/command.ts";
import { consola } from "../../shared/logger.ts";
import { WORKSPACE_ROOT } from "../../shared/repo-paths.ts";
import { type CargoAboutOutput, cargoEntries } from "./cargo.ts";
import { buildCreditsDocument } from "./document.ts";
import { readAcceptedLicenses } from "./license-policy.ts";
import {
  manualEntries,
  parseManualCredits,
  sidecarCoverage,
} from "./manual.ts";
import { npmEntries, type PnpmLicenses, readNpmPackages } from "./npm.ts";

const CREDITS_DIR = path.join(WORKSPACE_ROOT, "credits");
const ABOUT_CONFIG = path.join(CREDITS_DIR, "about.toml");
const OUTPUT = path.join(
  WORKSPACE_ROOT,
  "frontend/nyanpasu/src/assets/credits/open-source-credits.json",
);

async function run(
  program: string,
  args: string[],
  what: string,
): Promise<string> {
  let result: Awaited<ReturnType<typeof runCommand>>;
  try {
    result = await runCommand(program, { cwd: WORKSPACE_ROOT, args });
  } catch (error) {
    throw new Error(`failed to start ${what}`, { cause: error });
  }
  if (!result.success) {
    throw new Error(
      `${what} failed:
${result.stderr.toString("utf8")}`,
    );
  }
  return result.stdout.toString("utf8");
}

async function cargoAbout(): Promise<string> {
  // cargo-about refuses to write to a redirected stdout on Windows.
  const outputDir = await mkdtemp(path.join(tmpdir(), "nyanpasu-about-"));
  const output = path.join(outputDir, "about.json");
  try {
    await run("cargo", [
      "about",
      "generate",
      "--format",
      "json",
      "--locked",
      "--config",
      ABOUT_CONFIG,
      "--manifest-path",
      "backend/tauri/Cargo.toml",
      // Release builds use the defaults; nightly builds add these features.
      "--features",
      "nightly",
      "--output-file",
      output,
    ], "cargo-about (install it with `cargo binstall cargo-about@0.9.2`)");
    return await readFile(output, "utf8");
  } finally {
    await rm(outputDir, { recursive: true, force: true });
  }
}

function pnpmLicenses(): Promise<string> {
  const args = [
    "--filter",
    "@nyanpasu/nyanpasu...",
    "licenses",
    "list",
    "--prod",
    "--json",
  ];
  // pnpm is a .cmd shim on some Windows installs, which only a shell resolves.
  return process.platform === "win32"
    ? run("cmd", ["/c", "pnpm", ...args], "pnpm licenses list")
    : run("pnpm", args, "pnpm licenses list");
}

async function readNotices(dir: string): Promise<string[]> {
  const notices: string[] = [];
  const names: string[] = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (entry.isFile() && /^notice/i.test(entry.name)) names.push(entry.name);
  }
  for (const name of names.sort()) {
    notices.push(await readFile(path.join(dir, name), "utf8"));
  }
  return notices;
}

async function main(): Promise<number> {
  const { values: { check } } = parseArgs({
    options: { check: { type: "boolean" } },
    strict: false,
  });

  const accepted = readAcceptedLicenses(await readFile(ABOUT_CONFIG, "utf8"));
  const manual = parseManualCredits(
    await readFile(path.join(CREDITS_DIR, "components.toml"), "utf8"),
  );
  const { externalBin } = JSON.parse(
    await readFile(
      path.join(WORKSPACE_ROOT, "backend/tauri/tauri.conf.json"),
      "utf8",
    ),
  ).bundle as { externalBin: string[] };

  const [about, pnpm] = await Promise.all([cargoAbout(), pnpmLicenses()]);
  const aboutOutput = JSON.parse(about) as CargoAboutOutput;
  const notices = new Map<string, string[]>();
  for (const { package: pkg } of aboutOutput.crates) {
    if (pkg.source == null) continue;
    const found = await readNotices(path.dirname(pkg.manifest_path));
    if (found.length > 0) notices.set(pkg.id, found);
  }
  const npmPackages = await readNpmPackages(JSON.parse(pnpm) as PnpmLicenses, {
    readTextFile: (file) => readFile(file, "utf8"),
    readDirNames: async (dir) => {
      const entries = await readdir(dir, { withFileTypes: true });
      return entries.filter((entry) => entry.isFile()).map((entry) =>
        entry.name
      );
    },
  });

  const results = [
    manualEntries(manual.component, (file) => {
      try {
        return readFileSync(path.join(CREDITS_DIR, file), "utf8");
      } catch {
        return undefined;
      }
    }),
    cargoEntries(aboutOutput, notices),
    npmEntries(npmPackages, manual["npm-override"], accepted),
  ];
  const problems = [
    ...sidecarCoverage(externalBin, manual.component),
    ...results.flatMap((result) => result.problems),
  ];
  if (problems.length > 0) {
    consola.error(
      `open source credits are incomplete:\n${problems.join("\n")}`,
    );
    return 1;
  }

  const document = await buildCreditsDocument(
    results.flatMap((result) => result.entries),
  );
  const summary = `${document.packages.length} packages, ${
    Object.keys(document.texts).length
  } license texts`;
  if (check) {
    consola.success(`open source credits are complete: ${summary}`);
    return 0;
  }

  await mkdir(path.dirname(OUTPUT), { recursive: true });
  await writeFile(OUTPUT, JSON.stringify(document));
  consola.success(`open source credits generated: ${summary}`);
  return 0;
}

if (import.meta.main) {
  main().then((code) => process.exit(code)).catch((error) => {
    consola.error(error);
    process.exit(1);
  });
}
