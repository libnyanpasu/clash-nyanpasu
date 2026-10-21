import { parse as parseToml } from "jsr:@std/toml@1";
import { z } from "npm:zod";
import type { CreditEntry } from "./document.ts";
import {
  licenseIds,
  parseLicense,
  standardLicenseText,
} from "./license-policy.ts";

const componentSchema = z.strictObject({
  name: z.string().min(1),
  kind: z.enum(["sidecar", "resource", "data"]),
  homepage: z.url(),
  license: z.string().min(1).optional(),
  "license-files": z.array(z.string().min(1)).min(1).optional(),
  bundles: z.array(z.string().min(1)).min(1).optional(),
  platforms: z.array(z.enum(["windows", "linux", "macos"])).min(1).optional(),
  notes: z.string().min(1).optional(),
});

const npmOverrideSchema = z.strictObject({
  name: z.string().min(1),
  version: z.string().min(1),
  license: z.string().min(1),
  notes: z.string().min(1).optional(),
});

const manualCreditsSchema = z.strictObject({
  component: z.array(componentSchema).default([]),
  "npm-override": z.array(npmOverrideSchema).default([]),
});

export type ManualComponent = z.infer<typeof componentSchema>;
export type NpmOverride = z.infer<typeof npmOverrideSchema>;
export type ManualCredits = z.infer<typeof manualCreditsSchema>;

/** Parses `credits/components.toml`, naming the offending field on failure. */
export function parseManualCredits(toml: string): ManualCredits {
  const result = manualCreditsSchema.safeParse(parseToml(toml));
  if (!result.success) {
    throw new Error(
      `credits/components.toml is invalid:\n${z.prettifyError(result.error)}`,
    );
  }
  return result.data;
}

/**
 * Turns the hand-maintained components into credit entries. A component
 * without a license only declares where it comes from; one with a license
 * needs its text, from `license-files` or the SPDX reference text.
 */
export function manualEntries(
  components: ManualComponent[],
  readLicenseFile: (path: string) => string | undefined,
): { entries: CreditEntry[]; problems: string[] } {
  const entries: CreditEntry[] = [];
  const problems: string[] = [];
  const names = new Set<string>();

  for (const component of components) {
    const { name, license, "license-files": licenseFiles, ...rest } = component;
    const id = `component ${name}`;
    if (names.has(name)) problems.push(`${id} is declared twice`);
    names.add(name);

    const texts: string[] = [];
    if (license === undefined) {
      if (licenseFiles) problems.push(`${id} has license-files but no license`);
    } else {
      const node = parseLicense(license);
      if (!node) {
        problems.push(`${id} license ${license} is not valid SPDX`);
        continue;
      }
      for (const path of licenseFiles ?? []) {
        const text = readLicenseFile(path);
        if (text === undefined) problems.push(`${id} misses ${path}`);
        else texts.push(text);
      }
      if (!licenseFiles) {
        for (const licenseId of licenseIds(node)) {
          const text = standardLicenseText(licenseId);
          if (text === undefined) {
            problems.push(`${id} needs license-files for ${licenseId}`);
          } else texts.push(text);
        }
      }
    }

    const { bundles: _bundles, ...display } = rest;
    entries.push({ source: "bundled", name, license, texts, ...display });
  }
  return { entries, problems };
}

/** Every packaged sidecar must be credited, and every credited one packaged. */
export function sidecarCoverage(
  externalBin: string[],
  components: ManualComponent[],
): string[] {
  const covered = components.flatMap((component) => component.bundles ?? []);
  return [
    ...externalBin.filter((bin) => !covered.includes(bin)).map((bin) =>
      `${bin} is packaged but no component lists it in bundles`
    ),
    ...covered.filter((bin) => !externalBin.includes(bin)).map((bin) =>
      `bundles lists ${bin}, which tauri.conf.json does not package`
    ),
  ];
}
