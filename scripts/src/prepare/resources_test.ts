import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'
import { runCommand } from '../shared/command.ts'
import { normalizeArch, normalizePlatform } from './platform.ts'
import {
  assertMeowVersion,
  resolveSidecar,
  type ResourceResolverOptions,
} from './resources.ts'
import type { BinInfo } from './types.ts'

test('Meow version validation accepts matching stable and alpha output', () => {
  assertMeowVersion('Meow Meta 0.22.0', 'v0.22.0')
  assertMeowVersion('Meow Meta 0.22.0-alpha+3c27aca', 'alpha-3c27aca')
  assert.throws(
    () => assertMeowVersion('Meow Meta 0.22.0-alpha+deadbee', 'alpha-3c27aca'),
    { message: /unexpected Meow alpha version/ },
  )
})

async function createMeowTarball(
  tempRoot: string,
  name: string,
  platform: string,
): Promise<string> {
  const stageDir = path.join(tempRoot, 'fixture')
  const assetDir = path.join(stageDir, name.replace(/\.tar\.gz$/, ''))
  await mkdir(assetDir, { recursive: true })
  const magic =
    platform === 'linux'
      ? new Uint8Array([0x7f, 0x45, 0x4c, 0x46])
      : platform === 'win32'
        ? new Uint8Array([0x4d, 0x5a, 0x90, 0x00])
        : new Uint8Array([0xcf, 0xfa, 0xed, 0xfe])
  await writeFile(
    path.join(assetDir, 'meow'),
    new Uint8Array([...magic, ...new TextEncoder().encode('fixture')]),
    { mode: 0o755 },
  )

  const archive = path.join(tempRoot, name)
  const result = await runCommand('tar', {
    args: ['-czf', archive, '-C', stageDir, path.basename(assetDir)],
    stdout: 'piped',
    stderr: 'piped',
  })
  if (result.code !== 0) {
    throw new Error(result.stderr.toString('utf8'))
  }
  return archive
}

function foreignExecutableTarget(): { platform: string; arch: string } {
  const platform = process.platform === 'linux' ? 'darwin' : 'linux'
  return { platform, arch: 'x64' }
}

function nativeExecutableTarget(): { platform: string; arch: string } {
  return {
    platform: normalizePlatform(process.platform),
    arch: normalizeArch(process.arch),
  }
}

function resolverContext(
  root: string,
  target: { platform: string; arch: string },
): ResourceResolverOptions {
  return {
    resourcesDir: path.join(root, 'resources'),
    sidecarDir: path.join(root, 'sidecar'),
    tempRoot: path.join(root, 'temp'),
    ...target,
  }
}

function meowInfo(name: string, version: string, assetName: string): BinInfo {
  return {
    name,
    version,
    targetFile: `${name}-x86_64-unknown-linux-musl`,
    tmpFile: assetName,
    exeFile: `${assetName.replace(/\.tar\.gz$/, '')}/meow`,
    downloadURL: 'unused-because-the-test-seeds-the-cache',
  }
}

test('Meow repairs an unstamped legacy sidecar from the archive executable', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const target = foreignExecutableTarget()
  const context = resolverContext(root, target)
  const assetName = 'meow-v0.22.0-x86_64-unknown-linux-musl.tar.gz'
  const tempFile = path.join(context.tempRoot, 'meow', assetName)
  await mkdir(path.dirname(tempFile), { recursive: true })
  await createMeowTarball(path.dirname(tempFile), assetName, target.platform)
  await mkdir(context.sidecarDir, { recursive: true })
  const sidecarPath = path.join(
    context.sidecarDir,
    'meow-x86_64-unknown-linux-musl',
  )
  await writeFile(sidecarPath, new Uint8Array([0x1f, 0x8b, 0x08]))

  try {
    const result = await resolveSidecar(
      context,
      meowInfo('meow', 'v0.22.0', assetName),
    )
    assert.deepStrictEqual(result.cached, false)
    assert.deepStrictEqual(
      Array.from((await readFile(sidecarPath)).slice(0, 4)),
      target.platform === 'linux'
        ? [0x7f, 0x45, 0x4c, 0x46]
        : [0xcf, 0xfa, 0xed, 0xfe],
    )
    assert.deepStrictEqual(
      (await readFile(`${sidecarPath}.version`, 'utf8')).trim(),
      'v0.22.0',
    )
    assert.deepStrictEqual(
      (await resolveSidecar(context, meowInfo('meow', 'v0.22.0', assetName)))
        .cached,
      true,
    )
  } finally {
    await rm(root, { recursive: true })
  }
})

test('Meow Alpha version mismatch keeps the previous binary and stamp', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const target = nativeExecutableTarget()
  const context = resolverContext(root, target)
  const assetName = `meow-alpha-3c27aca-${target.platform}-native.tar.gz`
  const tempDir = path.join(context.tempRoot, 'meow-alpha')
  await mkdir(tempDir, { recursive: true })
  await createMeowTarball(tempDir, assetName, target.platform)
  await mkdir(context.sidecarDir, { recursive: true })
  const sidecarPath = path.join(context.sidecarDir, 'meow-alpha-native')
  await writeFile(sidecarPath, 'previous-good-binary')
  await writeFile(`${sidecarPath}.version`, 'alpha-3c27aca')
  const info = {
    ...meowInfo('meow-alpha', 'alpha-3c27aca', assetName),
    targetFile: 'meow-alpha-native',
  }
  context.runMeowVersion = async () => 'Meow Meta 0.22.0-alpha+deadbee'

  try {
    await assert.rejects(() => resolveSidecar(context, info, { force: true }), {
      message: /unexpected Meow alpha version/,
    })
    assert.deepStrictEqual(
      await readFile(sidecarPath, 'utf8'),
      'previous-good-binary',
    )
    assert.deepStrictEqual(
      await readFile(`${sidecarPath}.version`, 'utf8'),
      'alpha-3c27aca',
    )
  } finally {
    await rm(root, { recursive: true })
  }
})

test('Sidecar refreshes a gone alpha asset from the live release', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const context = resolverContext(root, foreignExecutableTarget())
  const originalFetch = globalThis.fetch
  let refreshCalls = 0
  globalThis.fetch = (input) => {
    const url = String(input)
    if (url === 'https://example.test/stale-alpha') {
      return Promise.resolve(new Response('gone', { status: 404 }))
    }
    if (url === 'https://example.test/live-alpha') {
      return Promise.resolve(new Response(new Uint8Array([0x01, 0x02, 0x03])))
    }
    return Promise.reject(new Error(`unexpected fetch: ${url}`))
  }

  const targetFile = 'clash-rs-alpha-fixture'
  const pinned: BinInfo = {
    name: 'clash-rs-alpha',
    version: 'alpha-stale',
    targetFile,
    tmpFile: 'clash-rs-alpha-stale',
    exeFile: 'clash-rs-alpha-stale',
    downloadURL: 'https://example.test/stale-alpha',
    refresh: () => {
      refreshCalls++
      return Promise.resolve({
        ...pinned,
        version: 'alpha-live',
        tmpFile: 'clash-rs-alpha-live',
        exeFile: 'clash-rs-alpha-live',
        downloadURL: 'https://example.test/live-alpha',
      })
    },
  }

  try {
    const result = await resolveSidecar(context, pinned)
    assert.deepStrictEqual(refreshCalls, 1)
    assert.deepStrictEqual(result.cached, false)
    assert.deepStrictEqual(result.version, 'alpha-live')
    assert.deepStrictEqual(result.size, 3)
    assert.deepStrictEqual(
      new Uint8Array(await readFile(path.join(context.sidecarDir, targetFile))),
      new Uint8Array([0x01, 0x02, 0x03]),
    )
  } finally {
    globalThis.fetch = originalFetch
    await rm(root, { recursive: true })
  }
})

test('Sidecar keeps the pinned asset while it is still available', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const context = resolverContext(root, foreignExecutableTarget())
  const originalFetch = globalThis.fetch
  let refreshCalls = 0
  globalThis.fetch = () =>
    Promise.resolve(new Response(new Uint8Array([0x04, 0x05])))

  const pinned: BinInfo = {
    name: 'clash-rs-alpha',
    version: 'alpha-pinned',
    targetFile: 'clash-rs-alpha-pinned',
    tmpFile: 'clash-rs-alpha-pinned',
    exeFile: 'clash-rs-alpha-pinned',
    downloadURL: 'https://example.test/pinned-alpha',
    refresh: () => {
      refreshCalls++
      return Promise.reject(new Error('refresh must not run'))
    },
  }

  try {
    const result = await resolveSidecar(context, pinned)
    assert.deepStrictEqual(refreshCalls, 0)
    assert.deepStrictEqual(result.version, 'alpha-pinned')
  } finally {
    globalThis.fetch = originalFetch
    await rm(root, { recursive: true })
  }
})

test('Sidecar reports the pinned download error when refresh fails', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const context = resolverContext(root, foreignExecutableTarget())
  const originalFetch = globalThis.fetch
  let refreshCalls = 0
  globalThis.fetch = () =>
    Promise.resolve(new Response('gone', { status: 404 }))

  const pinned: BinInfo = {
    name: 'meow-alpha',
    version: 'alpha-stale',
    targetFile: 'meow-alpha-fixture',
    tmpFile: 'meow-alpha-stale.tar.gz',
    exeFile: 'meow-alpha-stale/meow',
    downloadURL: 'https://example.test/stale-meow-alpha',
    refresh: () => {
      refreshCalls++
      return Promise.reject(new Error('live lookup failed'))
    },
  }

  try {
    await assert.rejects(() => resolveSidecar(context, pinned), {
      message: /download failed/,
    })
    assert.deepStrictEqual(refreshCalls, 1)
  } finally {
    globalThis.fetch = originalFetch
    await rm(root, { recursive: true })
  }
})
