// Extract all data-slot attribute values from frontend TSX files.
// Run: deno run -A scripts/extract-data-slots.ts
import { ensureDir } from "jsr:@std/fs@^1.0.19";
import { dirname, fromFileUrl, join } from "jsr:@std/path@^1.1.2";
import { globby } from "npm:globby";

const scriptDir = dirname(fromFileUrl(import.meta.url));
const workspaceRoot = join(scriptDir, "..");
const outputPath = join(
  workspaceRoot,
  "frontend/nyanpasu/src/generated/data-slots.gen.ts",
);

const files: string[] = await globby(["frontend/nyanpasu/src/**/*.tsx"], {
  cwd: workspaceRoot,
  absolute: true,
});
const slots = new Set<string>();

for (const file of files) {
  const content = await Deno.readTextFile(file);
  for (const match of content.matchAll(/data-slot="([^"]+)"/g)) {
    slots.add(match[1]);
  }
}

const sorted = [...slots].sort();
await ensureDir(dirname(outputPath));
await Deno.writeTextFile(
  outputPath,
  `// AUTO-GENERATED — do not edit manually. Run: pnpm generate:data-slots\n` +
    `export const DATA_SLOTS = ${
      JSON.stringify(sorted, null, 2)
    } as const\n\n` +
    `export type DataSlot = (typeof DATA_SLOTS)[number]\n`,
);
console.log(
  `[extract-data-slots] ${sorted.length} slots written to ${outputPath}`,
);
