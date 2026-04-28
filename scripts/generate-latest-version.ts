import { ensureDir } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
import {
  type ArchMapping,
  resolveClashPremium,
  resolveClashRs,
  resolveClashRsAlpha,
  resolveMihomo,
  resolveMihomoAlpha,
} from "./manifest.ts";
import { colorize, consola } from "./utils/logger.ts";

// === Constants ===

const WORKSPACE_ROOT = path.join(import.meta.dirname!, "..");
const MANIFEST_DIR = path.join(WORKSPACE_ROOT, "manifest");
const MANIFEST_VERSION_PATH = path.join(MANIFEST_DIR, "version.json");
const MANIFEST_VERSION = 1;

// === Types ===

type SupportedCore =
  | "mihomo"
  | "mihomo_alpha"
  | "clash_rs"
  | "clash_rs_alpha"
  | "clash_premium";

interface ManifestVersion {
  manifest_version: number;
  latest: Record<SupportedCore, string>;
  arch_template: Record<SupportedCore, ArchMapping>;
  updated_at?: string;
}

// === Main ===

const resolvers = [
  resolveMihomo,
  resolveMihomoAlpha,
  resolveClashRs,
  resolveClashPremium,
  resolveClashRsAlpha,
];

consola.start(colorize`{cyan Resolving} latest versions`);

const results = await Promise.all(resolvers.map((r) => r()));

consola.success("Resolved latest versions");
consola.start("Generating manifest");

const manifest: ManifestVersion = {
  manifest_version: MANIFEST_VERSION,
  latest: {} as Record<SupportedCore, string>,
  arch_template: {} as Record<SupportedCore, ArchMapping>,
};

for (const result of results) {
  manifest.latest[result.name as SupportedCore] = result.version;
  manifest.arch_template[result.name as SupportedCore] = result.archMapping;
}

await ensureDir(MANIFEST_DIR);

// If no changes, skip writing manifest
let previousManifest: Partial<ManifestVersion> = {};
try {
  previousManifest = JSON.parse(await Deno.readTextFile(MANIFEST_VERSION_PATH));
  delete previousManifest.updated_at;
} catch {
  // file may not exist yet
}

if (JSON.stringify(previousManifest) === JSON.stringify(manifest)) {
  consola.success("No changes, skip writing manifest");
  Deno.exit(0);
}

manifest.updated_at = new Date().toISOString();

consola.success("Generated manifest");

await Deno.writeTextFile(
  MANIFEST_VERSION_PATH,
  JSON.stringify(manifest, null, 2) + "\n",
);

consola.success("Manifest written");
