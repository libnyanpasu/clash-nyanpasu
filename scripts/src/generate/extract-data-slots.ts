// Extract all data-slot attribute values from frontend TSX files.
// Run: pnpm run generate:data-slots
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { globby } from 'globby'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'

const outputPath = join(
  WORKSPACE_ROOT,
  'frontend/nyanpasu/src/generated/data-slots.gen.ts',
)

const files: string[] = await globby(['frontend/*/src/**/*.tsx'], {
  cwd: WORKSPACE_ROOT,
  absolute: true,
})
const slots = new Set<string>()

for (const file of files) {
  const content = await readFile(file, 'utf8')
  for (const match of content.matchAll(/data-slot="([^"]+)"/g)) {
    slots.add(match[1])
  }
}

const sorted = [...slots].sort()
await mkdir(dirname(outputPath), { recursive: true })
await writeFile(
  outputPath,
  `// AUTO-GENERATED — do not edit manually. Run: pnpm run generate:data-slots\n` +
    `export const DATA_SLOTS = ${JSON.stringify(
      sorted,
      null,
      2,
    )} as const\n\n` +
    `export type DataSlot = (typeof DATA_SLOTS)[number]\n`,
)
console.log(
  `[extract-data-slots] ${sorted.length} slots written to ${outputPath}`,
)
