import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { exists } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
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
  let result: Deno.CommandOutput;
  try {
    result = await new Deno.Command(program, {
      cwd: WORKSPACE_ROOT,
      args,
      stdout: "piped",
      stderr: "piped",
    }).output();
  } catch (error) {
    throw new Error(`failed to start ${what}`, { cause: error });
  }
  if (!result.success) {
    throw new Error(
      `${what} failed:
${new TextDecoder().decode(result.stderr)}`,
    );
  }
  return new TextDecoder().decode(result.stdout);
}

async function cargoAbout(): Promise<string> {
  // cargo-about refuses to write to a redirected stdout on Windows.
  const output = await Deno.makeTempFile({ suffix: ".json" });
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
    return await Deno.readTextFile(output);
  } finally {
    await Deno.remove(output);
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
  return Deno.build.os === "windows"
    ? run("cmd", ["/c", "pnpm", ...args], "pnpm licenses list")
    : run("pnpm", args, "pnpm licenses list");
}

async function readNotices(dir: string): Promise<string[]> {
  const notices: string[] = [];
  const names: string[] = [];
  for await (const entry of Deno.readDir(dir)) {
    if (entry.isFile && /^notice/i.test(entry.name)) names.push(entry.name);
  }
  for (const name of names.sort()) {
    notices.push(await Deno.readTextFile(path.join(dir, name)));
  }
  return notices;
}

async function main(): Promise<number> {
  const { check } = parseArgs(Deno.args, { boolean: ["check"] });

  const accepted = readAcceptedLicenses(await Deno.readTextFile(ABOUT_CONFIG));
  const manual = parseManualCredits(
    await Deno.readTextFile(path.join(CREDITS_DIR, "components.toml")),
  );
  const { externalBin } = JSON.parse(
    await Deno.readTextFile(
      path.join(WORKSPACE_ROOT, "backend/tauri/tauri.conf.json"),
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
    readTextFile: (file) => Deno.readTextFile(file),
    readDirNames: async (dir) => {
      const names: string[] = [];
      for await (const entry of Deno.readDir(dir)) {
        if (entry.isFile) names.push(entry.name);
      }
      return names;
    },
  });

  const results = [
    manualEntries(manual.component, (file) => {
      try {
        return Deno.readTextFileSync(path.join(CREDITS_DIR, file));
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

  const outputDir = path.dirname(OUTPUT);
  if (!(await exists(outputDir))) await Deno.mkdir(outputDir);
  await Deno.writeTextFile(OUTPUT, JSON.stringify(document));
  consola.success(`open source credits generated: ${summary}`);
  return 0;
}

if (import.meta.main) {
  main().then((code) => Deno.exit(code)).catch((error) => {
    consola.error(error);
    Deno.exit(1);
  });
}
