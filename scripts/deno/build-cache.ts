import { parseArgs } from 'jsr:@std/cli@1/parse-args'
import { exists } from 'jsr:@std/fs'
import * as path from 'jsr:@std/path'
import {
  downloadCache,
  listCacheKeys,
  uploadCache,
} from './utils/cache-client.ts'
import { consola } from './utils/logger.ts'

// --- config ---

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..')
const TARGET_DIR = path.join(WORKSPACE_ROOT, 'backend/target')
const CARGO_LOCK_PATH = path.join(WORKSPACE_ROOT, 'backend/Cargo.lock')

const TAR_EXCLUDE_PATTERNS = [
  'bundle',
  '*.exe',
  '*.dmg',
  '*.deb',
  '*.rpm',
  '*.AppImage',
  '*.msi',
  '*.nsis',
]

// --- helpers ---

function requireEnv(name: string): string {
  const value = Deno.env.get(name)
  if (!value) {
    consola.fatal(`${name} is required`)
    Deno.exit(1)
  }
  return value
}

async function computeCargoLockHash(): Promise<string> {
  const content = await Deno.readFile(CARGO_LOCK_PATH)
  const hashBuffer = await crypto.subtle.digest('SHA-256', content)
  const hashArray = Array.from(new Uint8Array(hashBuffer))
  const hashHex = hashArray.map((b) => b.toString(16).padStart(2, '0')).join('')
  return hashHex.substring(0, 16)
}

function getCacheKey(os: string, arch: string, hash: string): string {
  return `nyanpasu-${os}-${arch}-${hash}`
}

function getFallbackPrefix(os: string, arch: string): string {
  return `nyanpasu-${os}-${arch}-`
}

async function createTarball(tarballPath: string): Promise<void> {
  consola.info(`creating tarball from ${TARGET_DIR}...`)

  const excludeArgs = TAR_EXCLUDE_PATTERNS.flatMap((p) => ['--exclude', p])

  const cmd = new Deno.Command('tar', {
    args: [
      '--zstd',
      '-cf',
      tarballPath,
      ...excludeArgs,
      '-C',
      path.dirname(TARGET_DIR),
      path.basename(TARGET_DIR),
    ],
    stdout: 'inherit',
    stderr: 'inherit',
  })

  const { code } = await cmd.output()
  if (code !== 0) {
    throw new Error(`tar creation failed with exit code ${code}`)
  }

  const stat = await Deno.stat(tarballPath)
  consola.success(`tarball created: ${tarballPath} (${stat.size} bytes)`)
}

async function extractTarball(tarballPath: string): Promise<void> {
  consola.info(`extracting tarball to ${path.dirname(TARGET_DIR)}...`)

  const cmd = new Deno.Command('tar', {
    args: ['--zstd', '-xf', tarballPath, '-C', path.dirname(TARGET_DIR)],
    stdout: 'inherit',
    stderr: 'inherit',
  })

  const { code } = await cmd.output()
  if (code !== 0) {
    throw new Error(`tar extraction failed with exit code ${code}`)
  }

  consola.success('tarball extracted successfully')
}

// --- commands ---

async function save(os: string, arch: string): Promise<void> {
  const token = requireEnv('FILE_SERVER_TOKEN')

  if (!(await exists(TARGET_DIR))) {
    consola.warn(`target directory does not exist: ${TARGET_DIR}`)
    return
  }

  const hash = await computeCargoLockHash()
  const key = getCacheKey(os, arch, hash)
  const tarballPath = path.join(Deno.makeTempDirSync(), `${key}.tar.zst`)

  try {
    await createTarball(tarballPath)
    await uploadCache(key, tarballPath, token)
    consola.success(`cache saved with key: ${key}`)
  } finally {
    try {
      await Deno.remove(tarballPath)
    } catch {
      // ignore cleanup errors
    }
  }
}

async function restore(os: string, arch: string): Promise<void> {
  const token = requireEnv('FILE_SERVER_TOKEN')

  const hash = await computeCargoLockHash()
  const key = getCacheKey(os, arch, hash)
  const tarballPath = path.join(Deno.makeTempDirSync(), `${key}.tar.zst`)

  try {
    // try exact match first
    let hit = await downloadCache(key, tarballPath, token)

    if (!hit) {
      // fallback: find most recent cache with matching prefix
      const prefix = getFallbackPrefix(os, arch)
      const keys = await listCacheKeys(prefix, token)

      if (keys.length > 0) {
        const fallbackKey = keys[0] // server returns sorted by update time desc
        consola.info(`using fallback cache key: ${fallbackKey}`)
        hit = await downloadCache(fallbackKey, tarballPath, token)
      }
    }

    if (!hit) {
      consola.warn('no cache found, build will run from scratch')
      return
    }

    await extractTarball(tarballPath)
    consola.success('build cache restored successfully')
  } finally {
    try {
      await Deno.remove(tarballPath)
    } catch {
      // ignore cleanup errors
    }
  }
}

// --- main ---

function main(): Promise<void> {
  const args = parseArgs(Deno.args, {
    string: ['os', 'arch'],
  })

  const subcommand = args._[0] as string | undefined
  const os = args.os
  const arch = args.arch

  if (!subcommand || !['save', 'restore'].includes(subcommand)) {
    consola.error(
      'usage: build-cache.ts <save|restore> --os <os> --arch <arch>',
    )
    Deno.exit(1)
  }

  if (!os || !arch) {
    consola.error('--os and --arch are required')
    Deno.exit(1)
  }

  if (subcommand === 'save') {
    return save(os, arch)
  } else {
    return restore(os, arch)
  }
}

main().catch((error) => {
  consola.fatal(error)
  Deno.exit(1)
})
