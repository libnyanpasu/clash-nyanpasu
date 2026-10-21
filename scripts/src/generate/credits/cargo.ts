import type { CreditEntry } from "./document.ts";

interface CargoPackage {
  id: string;
  name: string;
  version: string;
  source: string | null;
  license: string | null;
  homepage: string | null;
  repository: string | null;
  manifest_path: string;
}

/** The parts of `cargo about generate --format json` the credits read. */
export interface CargoAboutOutput {
  crates: { package: CargoPackage; license: string }[];
  licenses: { text: string; used_by: { crate: { id: string } }[] }[];
}

/** Third-party crates; path dependencies are this repository's own code. */
export function thirdPartyCrates(about: CargoAboutOutput): CargoPackage[] {
  return about.crates.map(({ package: pkg }) => pkg).filter((pkg) =>
    pkg.source != null
  );
}

/**
 * Converts `cargo-about` output into credit entries. `cargo-about` already
 * enforced the accepted licenses; `notices` holds each crate's NOTICE files,
 * which it does not collect.
 */
export function cargoEntries(
  about: CargoAboutOutput,
  notices: Map<string, string[]>,
): { entries: CreditEntry[]; problems: string[] } {
  const textsByCrate = new Map<string, string[]>();
  for (const { text, used_by } of about.licenses) {
    for (const { crate } of used_by) {
      textsByCrate.set(crate.id, [...textsByCrate.get(crate.id) ?? [], text]);
    }
  }

  const entries: CreditEntry[] = [];
  const problems: string[] = [];
  for (const { package: pkg, license } of about.crates) {
    if (pkg.source == null) continue;
    const texts = textsByCrate.get(pkg.id) ?? [];
    if (texts.length === 0) {
      problems.push(
        `cargo: ${pkg.name}@${pkg.version} has no license text (${license})`,
      );
    }
    entries.push({
      source: "cargo",
      name: pkg.name,
      version: pkg.version,
      homepage: pkg.homepage ?? pkg.repository ?? undefined,
      license: (pkg.license ?? license).replace(/\s+/g, " "),
      texts: [...texts, ...notices.get(pkg.id) ?? []],
    });
  }
  return { entries, problems };
}
