import { join } from "jsr:@std/path";
import type { CreditEntry } from "./document.ts";
import {
  chooseLicenses,
  parseLicense,
  standardLicenseText,
} from "./license-policy.ts";
import type { NpmOverride } from "./manual.ts";

/** `pnpm licenses list --json` output, grouped by license. */
export type PnpmLicenses = Record<
  string,
  { name: string; paths: string[]; license: string; homepage?: string }[]
>;

export interface NpmPackage {
  name: string;
  version: string;
  license: string;
  homepage?: string;
  /** The package's own license and notice files. */
  texts: string[];
}

export interface PackageFiles {
  readTextFile(path: string): Promise<string>;
  readDirNames(path: string): Promise<string[]>;
}

const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;

/**
 * Reads every installed copy once. pnpm groups versions per name, and its
 * peer-dependency variants share a version, so each directory's own
 * package.json decides the version.
 */
export async function readNpmPackages(
  pnpm: PnpmLicenses,
  files: PackageFiles,
): Promise<NpmPackage[]> {
  const packages = new Map<string, NpmPackage>();
  for (const { name, paths, license, homepage } of Object.values(pnpm).flat()) {
    for (const dir of paths) {
      const { version } = JSON.parse(
        await files.readTextFile(join(dir, "package.json")),
      ) as { version: string };
      const key = `${name}@${version}`;
      if (packages.has(key)) continue;

      const texts: string[] = [];
      for (const file of (await files.readDirNames(dir)).sort()) {
        if (LICENSE_FILE.test(file)) {
          texts.push(await files.readTextFile(join(dir, file)));
        }
      }
      packages.set(key, { name, version, license, homepage, texts });
    }
  }
  return [...packages.values()];
}

/**
 * Applies overrides and the accepted-license policy. Packages that ship no
 * license file fall back to the SPDX reference text of the chosen licenses.
 */
export function npmEntries(
  packages: NpmPackage[],
  overrides: NpmOverride[],
  accepted: string[],
): { entries: CreditEntry[]; problems: string[] } {
  const entries: CreditEntry[] = [];
  const problems: string[] = [];
  const used = new Set<NpmOverride>();

  for (const pkg of packages) {
    const id = `npm: ${pkg.name}@${pkg.version}`;
    const override = overrides.find((candidate) =>
      candidate.name === pkg.name && candidate.version === pkg.version
    );
    if (override) used.add(override);
    const license = override?.license ?? pkg.license;

    const node = license === "Unknown" ? null : parseLicense(license);
    if (!node) {
      problems.push(
        `${id} declares no valid license (${license}); add an [[npm-override]]`,
      );
      continue;
    }
    const chosen = chooseLicenses(node, accepted);
    if (!chosen) {
      problems.push(`${id} license ${license} is not accepted`);
      continue;
    }

    let texts = pkg.texts;
    if (texts.length === 0) {
      const standard = chosen.map(standardLicenseText);
      if (standard.some((text) => text === undefined)) {
        problems.push(`${id} ships no license file for ${license}`);
        continue;
      }
      texts = standard as string[];
    }
    entries.push({
      source: "npm",
      name: pkg.name,
      version: pkg.version,
      homepage: pkg.homepage,
      license,
      texts,
      notes: override?.notes,
    });
  }

  for (const override of overrides) {
    if (!used.has(override)) {
      problems.push(
        `[[npm-override]] ${override.name}@${override.version} matches no installed package`,
      );
    }
  }
  return { entries, problems };
}
