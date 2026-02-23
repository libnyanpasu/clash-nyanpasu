/**
 * Create and upload Windows portable package (zip)
 * Only runs on Windows
 */
import * as path from 'jsr:@std/path'
import { exists } from 'jsr:@std/fs'
// deno-lint-ignore no-explicit-any
import AdmZip from 'npm:adm-zip'
import { consola } from './utils/logger.ts'

if (Deno.build.os !== 'windows') {
  consola.info('Portable build is only supported on Windows, skipping.')
  Deno.exit(0)
}

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..')
const TAURI_APP_DIR = path.join(WORKSPACE_ROOT, 'backend/tauri')

const GITHUB_TOKEN = Deno.env.get('GITHUB_TOKEN') || Deno.env.get('GH_TOKEN')
const RUST_ARCH = Deno.env.get('RUST_ARCH') || 'x86_64'
const TAG_NAME = Deno.env.get('TAG_NAME')
const fixedWebview = Deno.args.includes('--fixed-webview')

if (!GITHUB_TOKEN) {
  consola.fatal('GITHUB_TOKEN is not set')
  Deno.exit(1)
}

if (!TAG_NAME) {
  consola.fatal('TAG_NAME is not set')
  Deno.exit(1)
}

async function resolvePortable(): Promise<void> {
  const buildDir =
    RUST_ARCH === 'x86_64'
      ? path.join(WORKSPACE_ROOT, 'backend/target/release')
      : path.join(
          WORKSPACE_ROOT,
          `backend/target/${RUST_ARCH}-pc-windows-msvc/release`,
        )

  const configDir = path.join(buildDir, '.config')

  if (!(await exists(buildDir))) {
    throw new Error(`Release dir not found: ${buildDir}`)
  }

  await Deno.mkdir(configDir, { recursive: true })
  await Deno.writeTextFile(path.join(configDir, 'PORTABLE'), '')

  const zip = new AdmZip()

  // Main executable
  let mainExe = path.join(buildDir, 'Clash Nyanpasu.exe')
  if (!(await exists(mainExe))) {
    mainExe = path.join(buildDir, 'clash-nyanpasu.exe')
  }
  zip.addLocalFile(mainExe)

  // Sidecar executables
  for (const exe of [
    'clash.exe',
    'mihomo.exe',
    'mihomo-alpha.exe',
    'nyanpasu-service.exe',
    'clash-rs.exe',
    'clash-rs-alpha.exe',
  ]) {
    const p = path.join(buildDir, exe)
    if (await exists(p)) {
      zip.addLocalFile(p)
    }
  }

  // Resources folder
  zip.addLocalFolder(path.join(buildDir, 'resources'), 'resources')

  // Fixed WebView2 runtime
  if (fixedWebview) {
    const files = await Array.fromAsync(Deno.readDir(TAURI_APP_DIR))
    const webviewEntry = files.find((f) => f.name.includes('WebView2'))
    if (!webviewEntry) {
      throw new Error('WebView2 runtime not found in tauri dir')
    }
    zip.addLocalFolder(
      path.join(TAURI_APP_DIR, webviewEntry.name),
      webviewEntry.name,
    )
  }

  zip.addLocalFolder(configDir, '.config')

  // Read version from package.json
  const pkgJson = JSON.parse(
    await Deno.readTextFile(path.join(WORKSPACE_ROOT, 'package.json')),
  ) as { version: string }
  const { version } = pkgJson

  const zipFile = `Clash.Nyanpasu_${version}_${RUST_ARCH}${fixedWebview ? '_fixed-webview' : ''}_portable.zip`
  const zipPath = path.join(WORKSPACE_ROOT, zipFile)
  zip.writeZip(zipPath)

  consola.success(`Created portable zip: ${zipFile}`)

  // Upload to GitHub release
  const cmd = new Deno.Command('gh', {
    args: ['release', 'upload', TAG_NAME, zipPath, '--clobber'],
    stdout: 'piped',
    stderr: 'piped',
    env: {
      GH_TOKEN: GITHUB_TOKEN!,
      GITHUB_TOKEN: GITHUB_TOKEN!,
    },
  })

  const output = await cmd.output()
  if (output.code !== 0) {
    const stderr = new TextDecoder().decode(output.stderr)
    consola.error(stderr)
    throw new Error(`Failed to upload portable zip to release ${TAG_NAME}`)
  }

  consola.success(`Uploaded ${zipFile} to release ${TAG_NAME}`)
}

resolvePortable().catch((err) => {
  consola.error(err)
  Deno.exit(1)
})
