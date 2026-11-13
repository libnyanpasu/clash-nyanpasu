import { mkdir, readFile, writeFile } from 'node:fs/promises'
import * as path from 'node:path'
import { colorize, consola } from '../shared/logger.ts'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'
import {
  createVersionManifest,
  resolveClashRs,
  resolveClashRsAlpha,
  resolveMeow,
  resolveMeowAlpha,
  resolveMihomo,
  resolveMihomoAlpha,
  type VersionManifest,
} from './manifest.ts'

// === Constants ===

const MANIFEST_DIR = path.join(WORKSPACE_ROOT, 'manifest')
const MANIFEST_VERSION_PATH = path.join(MANIFEST_DIR, 'version.json')

// === Main ===

const resolvers = [
  resolveMihomo,
  resolveMihomoAlpha,
  resolveClashRs,
  resolveClashRsAlpha,
  resolveMeow,
  resolveMeowAlpha,
]

consola.start(colorize`{cyan Resolving} latest versions`)

const results = await Promise.all(resolvers.map((r) => r()))

consola.success('Resolved latest versions')
consola.start('Generating manifest')

const manifest = createVersionManifest(results)

await mkdir(MANIFEST_DIR, { recursive: true })

// If no changes, skip writing manifest
let previousManifest: Partial<VersionManifest> = {}
try {
  previousManifest = JSON.parse(await readFile(MANIFEST_VERSION_PATH, 'utf8'))
  delete previousManifest.updated_at
} catch {
  // file may not exist yet
}

if (JSON.stringify(previousManifest) === JSON.stringify(manifest)) {
  consola.success('No changes, skip writing manifest')
  process.exit(0)
}

manifest.updated_at = new Date().toISOString()

consola.success('Generated manifest')

await writeFile(MANIFEST_VERSION_PATH, JSON.stringify(manifest, null, 2) + '\n')

consola.success('Manifest written')
