import assert from 'node:assert/strict'
import {
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'
import type { downloadFile } from './download.ts'
import {
  enableLoopbackInfo,
  getEnableLoopbackInfo,
  parseEnableLoopbackChecksum,
  parseEnableLoopbackVersion,
  verifyEnableLoopbackChecksum,
} from './enable-loopback.ts'
import { resolveEnableLoopback } from './resources.ts'

const BYTES = new TextEncoder().encode('abc')
const HASH = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'

test('enable-loopback reads only the package version, including CRLF and prereleases', async () => {
  assert.deepStrictEqual(
    parseEnableLoopbackVersion(
      '[package]\r\nname = "tool"\r\nversion = "1.2.3-rc.1" # pin\r\n[dependencies]\r\n',
    ),
    '1.2.3-rc.1',
  )
  for (const manifest of [
    '[dependencies]\nversion = "9.9.9"',
    '[package]\nversion.workspace = true\n[dependencies]\nversion = "9.9.9"',
    '[package]\nversion = "latest"',
  ]) {
    assert.throws(() => parseEnableLoopbackVersion(manifest))
  }
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  try {
    const dir = path.join(
      root,
      'backend/enable-loopback/crates/enable-loopback',
    )
    await mkdir(dir, { recursive: true })
    await writeFile(
      path.join(dir, 'Cargo.toml'),
      "[package]\nversion = '2.3.4'\n",
    )
    assert.deepStrictEqual(
      await getEnableLoopbackInfo(root, 'arm64'),
      enableLoopbackInfo('2.3.4', 'arm64'),
    )
  } finally {
    await rm(root, { recursive: true })
  }
})

test('enable-loopback release URLs follow both packaging targets', () => {
  for (const [arch, target] of [
    ['x64', 'x86_64'],
    ['arm64', 'aarch64'],
  ]) {
    const info = enableLoopbackInfo('0.1.0', arch)
    assert.deepStrictEqual(
      info.downloadURL,
      `https://github.com/libnyanpasu/enable-loopback/releases/download/v0.1.0/enableLoopback-${target}-pc-windows-msvc.exe`,
    )
    assert.deepStrictEqual(info.checksumURL, `${info.downloadURL}.sha256`)
  }
  assert.throws(() => enableLoopbackInfo('0.1.0', 'ia32'))
})

test('enable-loopback validates sha256sum format, filename and digest', async () => {
  const name = enableLoopbackInfo('0.1.0', 'x64').assetName
  assert.deepStrictEqual(
    parseEnableLoopbackChecksum(`${HASH.toUpperCase()}  ${name}\r\n`, name),
    HASH,
  )
  for (const checksum of [
    HASH,
    `${HASH}  other.exe`,
    `${HASH}  ${name}\n${HASH}  ${name}`,
    `bad  ${name}`,
  ]) {
    assert.throws(() => parseEnableLoopbackChecksum(checksum, name))
  }
  await verifyEnableLoopbackChecksum(BYTES, `${HASH}  ${name}\n`, name)
  await assert.rejects(
    () =>
      verifyEnableLoopbackChecksum(
        new Uint8Array([0]),
        `${HASH}  ${name}`,
        name,
      ),
    { message: /SHA-256 mismatch/ },
  )
})

test('enable-loopback installs verified bytes, replaces legacy cache and preserves it on mismatch', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-scripts-'))
  const context = {
    resourcesDir: path.join(root, 'resources'),
    sidecarDir: path.join(root, 'sidecar'),
    tempRoot: path.join(root, 'temp'),
    platform: 'win32',
    arch: 'x64',
  }
  const info = enableLoopbackInfo('0.1.0', 'x64')
  let corrupt = false
  let executableDownloads = 0
  const download: typeof downloadFile = async (url, destination) => {
    if (url === info.checksumURL) {
      await writeFile(destination, `${HASH}  ${info.assetName}\n`)
    } else {
      assert.deepStrictEqual(url, info.downloadURL)
      executableDownloads++
      await writeFile(destination, corrupt ? new Uint8Array([0]) : BYTES)
    }
    return { size: 3, speed: 1 }
  }
  try {
    await mkdir(context.resourcesDir)
    const target = path.join(context.resourcesDir, 'enableLoopback.exe')
    await writeFile(target, 'legacy tool')
    assert.deepStrictEqual(
      (await resolveEnableLoopback(context, info, undefined, download)).cached,
      false,
    )
    assert.deepStrictEqual(new Uint8Array(await readFile(target)), BYTES)
    assert.deepStrictEqual(
      (await resolveEnableLoopback(context, info, undefined, download)).cached,
      true,
    )
    assert.deepStrictEqual(executableDownloads, 1)
    corrupt = true
    await assert.rejects(
      () => resolveEnableLoopback(context, info, { force: true }, download),
      { message: /SHA-256 mismatch/ },
    )
    assert.deepStrictEqual(new Uint8Array(await readFile(target)), BYTES)
    await writeFile(target, 'tampered')
    await assert.rejects(
      () => resolveEnableLoopback(context, info, undefined, download),
      { message: /SHA-256 mismatch/ },
    )
    assert.deepStrictEqual(await readdir(context.tempRoot), [])
  } finally {
    await rm(root, { recursive: true })
  }
})
