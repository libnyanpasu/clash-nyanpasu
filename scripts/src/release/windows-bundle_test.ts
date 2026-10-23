import assert from 'node:assert/strict'
import {
  copyFile,
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
import { fileURLToPath } from 'node:url'
import AdmZip from 'adm-zip'
import { runCommand } from '../shared/command.ts'
import { createPortableArchive } from './portable.ts'

const FIXED_WEBVIEW_DIR = 'WebView2'

const tauriSource = new URL('../../../backend/tauri/', import.meta.url)

// Tauri merges config using JSON Merge Patch: objects merge, arrays replace.
function mergeConfig(...configs: Record<string, unknown>[]) {
  const result: Record<string, unknown> = {}
  for (const config of configs) {
    for (const [key, value] of Object.entries(config)) {
      if (value === null) {
        delete result[key]
      } else if (typeof value === 'object' && !Array.isArray(value)) {
        const previous = result[key]
        result[key] = mergeConfig(
          previous && typeof previous === 'object' && !Array.isArray(previous)
            ? (previous as Record<string, unknown>)
            : {},
          value as Record<string, unknown>,
        )
      } else {
        result[key] = value
      }
    }
  }
  return result
}

async function withFixture(run: (directory: string) => Promise<void>) {
  const directory = await mkdtemp(path.join(tmpdir(), 'nyanpasu-bundle-test-'))
  try {
    await mkdir(path.join(directory, 'overrides'), { recursive: true })
    for (const file of [
      'tauri.conf.json',
      'tauri.windows.conf.json',
      'overrides/fixed-webview2.conf.json',
      'overrides/nightly.conf.json',
    ]) {
      await copyFile(new URL(file, tauriSource), path.join(directory, file))
    }
    await run(directory)
  } finally {
    await rm(directory, { recursive: true })
  }
}

async function addRuntime(directory: string) {
  const runtime = path.join(directory, FIXED_WEBVIEW_DIR)
  await mkdir(path.join(runtime, 'locales'), { recursive: true })
  await writeFile(path.join(runtime, 'msedgewebview2.exe'), 'runtime')
  await writeFile(path.join(runtime, 'locales/en-US.pak'), 'locale')
}

test('fixed packaging changes only WebView2 mode and keeps shared update URLs', async () => {
  await withFixture(async (directory) => {
    const read = async (file: string) =>
      JSON.parse(await readFile(path.join(directory, file), 'utf8'))
    const base = await read('tauri.conf.json')
    const windows = await read('tauri.windows.conf.json')
    const fixed = await read('overrides/fixed-webview2.conf.json')
    const nightly = mergeConfig(base, await read('overrides/nightly.conf.json'))
    for (const channel of [{}, nightly]) {
      const compiled = mergeConfig(base, windows, channel)
      const bundled = mergeConfig(compiled, fixed)
      assert.deepStrictEqual(
        bundled,
        mergeConfig(compiled, {
          $schema: fixed.$schema,
          bundle: {
            windows: {
              webviewInstallMode: {
                type: 'fixedRuntime',
                path: FIXED_WEBVIEW_DIR,
              },
            },
          },
        }),
      )
    }
  })
})

test('shared release preparation preserves Linux and macOS config without Windows assets', async () => {
  await withFixture(async (directory) => {
    const tauriDir = path.join(directory, 'backend/tauri')
    await mkdir(tauriDir, { recursive: true })
    const manifest = '{"name":"release-fixture","version":"1.2.3"}\n'
    await writeFile(path.join(directory, 'package.json'), manifest)
    for (const targets of [
      ['deb', 'appimage', 'rpm'],
      ['app', 'dmg'],
    ]) {
      const config = {
        version: '1.2.3',
        bundle: { targets, resources: ['resources'] },
        plugins: {
          updater: { endpoints: ['https://example.com/update.json'] },
        },
      }
      await writeFile(
        path.join(tauriDir, 'tauri.conf.json'),
        JSON.stringify(config),
      )
      const result = await runCommand(process.execPath, {
        args: [fileURLToPath(new URL('prepare-release.ts', import.meta.url))],
        cwd: directory,
        stdout: 'piped',
        stderr: 'piped',
      })
      assert(result.success, result.stderr.toString('utf8'))
      assert.deepStrictEqual(
        JSON.parse(
          await readFile(path.join(tauriDir, 'tauri.conf.json'), 'utf8'),
        ),
        config,
      )
      assert.deepStrictEqual(
        await readFile(path.join(directory, 'package.json'), 'utf8'),
        manifest,
      )
      assert.deepStrictEqual(await readdir(tauriDir), ['tauri.conf.json'])
    }
  })
})

test('portable archives reuse the application and differ only by bundled runtime', async () => {
  await withFixture(async (directory) => {
    await addRuntime(directory)
    const buildDir = path.join(directory, 'target/release')
    await mkdir(path.join(buildDir, 'resources'), { recursive: true })
    const binaryNames = [
      'Clash Nyanpasu.exe',
      'clash.exe',
      'mihomo.exe',
      'mihomo-alpha.exe',
      'nyanpasu-service.exe',
      'clash-rs.exe',
      'clash-rs-alpha.exe',
    ]
    for (const name of binaryNames) {
      await writeFile(path.join(buildDir, name), `binary:${name}`)
    }
    await writeFile(path.join(buildDir, 'resources/geoip.dat'), 'geoip')
    for (const variant of [
      'fixed-webview',
      'standard',
      'fixed-webview',
    ] as const) {
      const archive = await createPortableArchive(buildDir, directory, variant)
      // Round-trip the actual ZIP bytes, not just the in-memory entry list.
      const zip = new AdmZip(archive.toBuffer())
      const expectedFiles = [
        ...binaryNames,
        '.config/PORTABLE',
        'resources/geoip.dat',
      ]
      if (variant === 'fixed-webview') {
        expectedFiles.push(
          'WebView2/msedgewebview2.exe',
          'WebView2/locales/en-US.pak',
        )
      }
      assert.deepStrictEqual(
        zip
          .getEntries()
          .filter((entry) => !entry.isDirectory)
          .map((entry) => entry.entryName)
          .sort(),
        expectedFiles.sort(),
      )
      assert(zip.getEntry('.config/PORTABLE'))
      assert.deepStrictEqual(zip.readAsText('resources/geoip.dat'), 'geoip')
      for (const name of binaryNames) {
        assert.deepStrictEqual(zip.readAsText(name), `binary:${name}`)
      }
      const runtimeFiles = zip
        .getEntries()
        .filter(
          (entry) =>
            entry.entryName.startsWith(`${FIXED_WEBVIEW_DIR}/`) &&
            !entry.isDirectory,
        )
        .map((entry) => entry.entryName)
        .sort()
      assert.deepStrictEqual(
        runtimeFiles,
        variant === 'fixed-webview'
          ? ['WebView2/locales/en-US.pak', 'WebView2/msedgewebview2.exe']
          : [],
      )
    }
    await rm(path.join(directory, FIXED_WEBVIEW_DIR), {
      recursive: true,
    })
    await assert.rejects(
      () => createPortableArchive(buildDir, directory, 'fixed-webview'),
      { message: /WebView2 runtime not found/ },
    )
    await createPortableArchive(buildDir, directory, 'standard')
    assert.deepStrictEqual(
      await readFile(path.join(buildDir, 'Clash Nyanpasu.exe'), 'utf8'),
      'binary:Clash Nyanpasu.exe',
    )
  })
})
