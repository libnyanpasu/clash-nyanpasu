// Extract all data-slot attribute values from frontend TSX files.
// Run: deno task generate:data-slots
import { ensureDir } from "jsr:@std/fs@^1.0.19";
import { dirname, join } from "jsr:@std/path@^1.1.2";
import { globby } from "npm:globby";
import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import { extractDataSlots } from "./data-slots.ts";

const outputPath = join(
  WORKSPACE_ROOT,
  "frontend/nyanpasu/src/generated/data-slots.gen.ts",
);

const files: string[] = await globby(["frontend/*/src/**/*.tsx"], {
  cwd: WORKSPACE_ROOT,
  absolute: true,
});
const slots = new Set<string>();

for (const file of files) {
  const content = await Deno.readTextFile(file);
  for (const slot of extractDataSlots(content)) {
    slots.add(slot);
  }
}

const sorted = [...slots].sort();
await ensureDir(dirname(outputPath));
await Deno.writeTextFile(
  outputPath,
  `// AUTO-GENERATED — do not edit manually. Run: deno task generate:data-slots\n` +
    `export const DATA_SLOTS = ${
      JSON.stringify(sorted, null, 2)
    } as const\n\n` +
    `export type DataSlot = (typeof DATA_SLOTS)[number]\n`,
);
console.log(
  `[extract-data-slots] ${sorted.length} slots written to ${outputPath}`,
);
