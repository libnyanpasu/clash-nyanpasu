// @ts-types="npm:@types/adm-zip"
import { parseArgs } from 'jsr:@std/cli@1/parse-args'
import { ensureDir, exists } from 'jsr:@std/fs'
import * as path from 'jsr:@std/path'
import AdmZip from 'npm:adm-zip'
// @ts-types="npm:@types/figlet"
import figlet from 'npm:figlet'
import { colorize, consola } from './utils/logger.ts'

// === Types ===

interface BinInfo {
  name: string
  targetFile: string
  exeFile: string
  tmpFile: string
  downloadURL: string
}

type SupportedArch =
  | 'windows-i386'
  | 'windows-x86_64'
  | 'windows-arm64'
  | 'linux-aarch64'
  | 'linux-amd64'
  | 'linux-i386'
  | 'linux-armv7'
  | 'linux-armv7hf'
  | 'darwin-arm64'
  | 'darwin-x64'

type ArchMapping = Record<SupportedArch, string>

interface ClashManifest {
  URL_PREFIX: string
  BACKUP_URL_PREFIX?: string
  BACKUP_LATEST_DATE?: string
  VERSION?: string
  VERSION_URL?: string
  ARCH_MAPPING: ArchMapping
}

// === Constants ===

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..')
const TAURI_APP_DIR = path.join(WORKSPACE_ROOT, 'backend/tauri')
const TEMP_DIR = path.join(WORKSPACE_ROOT, 'node_modules/.verge')

// === CLI Args ===

const args = parseArgs(Deno.args, {
  boolean: ['force'],
  string: ['arch', 'sidecar-host'],
})

const FORCE = args.force
const ARCH_OVERRIDE = args.arch

// === Platform detection ===

// Deno.build.os: 'windows' | 'darwin' | 'linux' | ...
// Map to Node-style for arch table compatibility
const platform = Deno.build.os === 'windows' ? 'win32' : Deno.build.os

// Deno.build.arch: 'x86_64' | 'aarch64'
// Map to Node-style for arch table compatibility
const DENO_ARCH_TO_NODE: Record<string, string> = {
  x86_64: 'x64',
  aarch64: 'arm64',
}
const arch =
  ARCH_OVERRIDE ?? DENO_ARCH_TO_NODE[Deno.build.arch] ?? Deno.build.arch

// === Sidecar Host ===

let SIDECAR_HOST = args['sidecar-host']
if (!SIDECAR_HOST) {
  const cmd = new Deno.Command('rustc', { args: ['-vV'], stdout: 'piped' })
  const { stdout } = await cmd.output()
  const text = new TextDecoder().decode(stdout)
  SIDECAR_HOST = text.match(/host: (.+)/)?.[1]?.trim()
}

if (!SIDECAR_HOST) {
  consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`)
  Deno.exit(1)
}

consola.debug(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`)
consola.debug(colorize`platform {yellow ${platform}}`)
consola.debug(colorize`arch {yellow ${arch}}`)

// === Arch Mapping ===

function mapArch(platform: string, arch: string): SupportedArch {
  const mapping: Partial<Record<string, SupportedArch>> = {
    'darwin-x64': 'darwin-x64',
    'darwin-arm64': 'darwin-arm64',
    'win32-x64': 'windows-x86_64',
    'win32-ia32': 'windows-i386',
    'win32-arm64': 'windows-arm64',
    'linux-x64': 'linux-amd64',
    'linux-ia32': 'linux-i386',
    'linux-arm': 'linux-armv7hf',
    'linux-arm64': 'linux-aarch64',
    'linux-armel': 'linux-armv7',
  }
  const result = mapping[`${platform}-${arch}`]
  if (!result) {
    throw new Error(`Unsupported platform/architecture: ${platform}-${arch}`)
  }
  return result
}

// === Version Manifest ===

const versionManifest = JSON.parse(
  await Deno.readTextFile(path.join(WORKSPACE_ROOT, 'manifest/version.json')),
)

const CLASH_MANIFEST: ClashManifest = {
  URL_PREFIX: 'https://github.com/Dreamacro/clash/releases/download/premium/',
  BACKUP_URL_PREFIX:
    'https://github.com/zhongfly/Clash-premium-backup/releases/download/',
  BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
  VERSION: versionManifest.latest.clash_premium,
  ARCH_MAPPING: versionManifest.arch_template.clash_premium as ArchMapping,
}

const CLASH_META_MANIFEST: ClashManifest = {
  URL_PREFIX: `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
  VERSION: versionManifest.latest.mihomo,
  ARCH_MAPPING: versionManifest.arch_template.mihomo as ArchMapping,
}

const CLASH_META_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt',
  URL_PREFIX:
    'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha',
  ARCH_MAPPING: versionManifest.arch_template.mihomo_alpha as ArchMapping,
}

const CLASH_RS_MANIFEST: ClashManifest = {
  URL_PREFIX: 'https://github.com/Watfaq/clash-rs/releases/download/',
  VERSION: versionManifest.latest.clash_rs,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs as ArchMapping,
}

const CLASH_RS_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    'https://github.com/Watfaq/clash-rs/releases/download/latest/version.txt',
  URL_PREFIX: 'https://github.com/Watfaq/clash-rs/releases/download/latest',
  ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha as ArchMapping,
}

// === Download ===

async function downloadFile(url: string, filePath: string): Promise<void> {
  consola.debug(colorize`downloading {gray "${url.split('/').at(-1)}"}`)

  const response = await fetch(url, {
    method: 'GET',
    headers: {
      'Content-Type': 'application/octet-stream',
      'User-Agent':
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0',
    },
  })

  if (!response.ok) {
    throw new Error(
      `download failed: ${response.statusText} (${response.status})`,
    )
  }

  const buffer = await response.arrayBuffer()
  await Deno.writeFile(filePath, new Uint8Array(buffer))
}

// === Extract Helpers ===

async function extractZip(
  zipPath: string,
  destDir: string,
  name: string,
): Promise<string> {
  const zip = new AdmZip(zipPath)
  const baseName = name
    .split('-')
    .filter((o: string) => o !== 'alpha')
    .join('-')
  let entryName: string | undefined

  for (const entry of zip.getEntries()) {
    consola.debug(colorize`"{green ${name}}" entry name ${entry.entryName}`)
    if (
      (entry.entryName.includes(name) && entry.entryName.endsWith('.exe')) ||
      (entry.entryName.includes(baseName) && entry.entryName.endsWith('.exe'))
    ) {
      entryName = entry.entryName
    }
  }

  zip.extractAllTo(destDir, true)

  if (!entryName) throw new Error('cannot find exe file in zip')

  return path.join(destDir, entryName)
}

async function extractTarGz(
  tarPath: string,
  destDir: string,
  name: string,
): Promise<void> {
  const cmd = new Deno.Command('tar', {
    args: ['-xzf', tarPath, '-C', destDir],
    stdout: 'piped',
    stderr: 'piped',
  })
  const { code, stderr } = await cmd.output()
  if (code !== 0) {
    throw new Error(
      `tar extraction failed: ${new TextDecoder().decode(stderr)}`,
    )
  }
}

async function gunzipFile(
  inputPath: string,
  outputPath: string,
): Promise<void> {
  const input = await Deno.open(inputPath, { read: true })
  const output = await Deno.open(outputPath, { write: true, create: true })
  await input.readable
    .pipeThrough(new DecompressionStream('gzip'))
    .pipeTo(output.writable)
}

// === Resource Resolution ===

async function resolveResource(
  binInfo: { file: string; downloadURL: string },
  options?: { force?: boolean },
): Promise<void> {
  const { file, downloadURL } = binInfo
  const resDir = path.join(TAURI_APP_DIR, 'resources')
  const targetPath = path.join(resDir, file)

  if (!options?.force && (await exists(targetPath))) return

  await ensureDir(resDir)
  await downloadFile(downloadURL, targetPath)

  consola.success(colorize`resolve {green ${file}} finished`)
}

async function resolveSidecar(
  binInfo: BinInfo | Promise<BinInfo>,
  options?: { force?: boolean },
): Promise<void> {
  const { name, targetFile, tmpFile, exeFile, downloadURL } = await binInfo

  const sidecarDir = path.join(TAURI_APP_DIR, 'sidecar')
  const sidecarPath = path.join(sidecarDir, targetFile)

  await ensureDir(sidecarDir)

  if (!options?.force && (await exists(sidecarPath))) return

  const tempDir = path.join(TEMP_DIR, name)
  const tempFile = path.join(tempDir, tmpFile)
  const tempExe = path.join(tempDir, exeFile)

  await ensureDir(tempDir)

  try {
    if (!(await exists(tempFile))) {
      await downloadFile(downloadURL, tempFile)
    }

    if (tmpFile.endsWith('.zip')) {
      const extractedExe = await extractZip(tempFile, tempDir, name)
      await Deno.rename(extractedExe, tempExe)
      await Deno.rename(tempExe, sidecarPath)
    } else if (tmpFile.endsWith('.tar.gz')) {
      await extractTarGz(tempFile, tempDir, name)
      await Deno.rename(tempExe, sidecarPath)
    } else if (tmpFile.endsWith('.gz')) {
      await gunzipFile(tempFile, sidecarPath)
      await Deno.chmod(sidecarPath, 0o755)
    } else {
      await Deno.rename(tempFile, sidecarPath)
      if (platform !== 'win32') {
        await Deno.chmod(sidecarPath, 0o755)
      }
    }

    consola.success(colorize`resolve {green ${name}} finished`)
  } catch (err) {
    try {
      await Deno.remove(sidecarPath)
    } catch {
      // ignore
    }
    throw err
  } finally {
    try {
      await Deno.remove(tempDir, { recursive: true })
    } catch {
      // ignore
    }
  }
}

// === Binary Info Functions ===

function getClashBackupInfo(): BinInfo {
  const { ARCH_MAPPING, BACKUP_URL_PREFIX, BACKUP_LATEST_DATE } = CLASH_MANIFEST
  const archLabel = mapArch(platform, arch)
  const name = ARCH_MAPPING[archLabel].replace('{}', BACKUP_LATEST_DATE!)
  const isWin = platform === 'win32'
  return {
    name: 'clash',
    targetFile: `clash-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: `${name}${isWin ? '.exe' : ''}`,
    tmpFile: name,
    downloadURL: `${BACKUP_URL_PREFIX}${BACKUP_LATEST_DATE}/${name}`,
  }
}

function getClashMetaInfo(): BinInfo {
  const { ARCH_MAPPING, URL_PREFIX, VERSION } = CLASH_META_MANIFEST
  const archLabel = mapArch(platform, arch)
  const name = ARCH_MAPPING[archLabel].replace('{}', VERSION!)
  const isWin = platform === 'win32'
  return {
    name: 'mihomo',
    targetFile: `mihomo-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: `${name}${isWin ? '.exe' : ''}`,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  }
}

async function getClashMetaAlphaInfo(): Promise<BinInfo> {
  const { ARCH_MAPPING, URL_PREFIX, VERSION_URL } = CLASH_META_ALPHA_MANIFEST
  const resp = await fetch(VERSION_URL!)
  const version = (await resp.text()).trim()
  consola.debug(`mihomo-alpha version: ${version}`)
  const archLabel = mapArch(platform, arch)
  const name = ARCH_MAPPING[archLabel].replace('{}', version)
  const isWin = platform === 'win32'
  return {
    name: 'mihomo-alpha',
    targetFile: `mihomo-alpha-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: `${name}${isWin ? '.exe' : ''}`,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  }
}

function getClashRustInfo(): BinInfo {
  const { ARCH_MAPPING, URL_PREFIX, VERSION } = CLASH_RS_MANIFEST
  const archLabel = mapArch(platform, arch)
  const name = ARCH_MAPPING[archLabel].replace('{}', VERSION!)
  const isWin = platform === 'win32'
  return {
    name: 'clash-rs',
    targetFile: `clash-rs-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: name,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}${VERSION}/${name}`,
  }
}

async function getClashRustAlphaInfo(): Promise<BinInfo> {
  const { ARCH_MAPPING, VERSION_URL, URL_PREFIX } = CLASH_RS_ALPHA_MANIFEST

  const resp = await fetch(VERSION_URL!)
  const version = (await resp.text()).trim()
  consola.debug(`clash-rs-alpha version: ${version}`)
  const archLabel = mapArch(platform, arch)
  const name = ARCH_MAPPING[archLabel].replace('{}', version)
  const isWin = platform === 'win32'
  return {
    name: 'clash-rs-alpha',
    targetFile: `clash-rs-alpha-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: name,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  }
}

async function getNyanpasuServiceInfo(): Promise<BinInfo> {
  const SERVICE_REPO = 'libnyanpasu/nyanpasu-service'
  const isWin = SIDECAR_HOST!.includes('windows')
  const urlExt = isWin ? 'zip' : 'tar.gz'

  const response = await fetch(
    `https://github.com/${SERVICE_REPO}/releases/latest`,
    { method: 'GET', redirect: 'manual' },
  )
  const location = response.headers.get('location')
  if (!location) throw new Error('Cannot find location from response header')
  const version = location.split('/').pop()
  if (!version) throw new Error('Cannot find tag from location')
  consola.debug(`nyanpasu-service version: ${version}`)

  const name = 'nyanpasu-service'
  return {
    name,
    targetFile: `${name}-${SIDECAR_HOST}${isWin ? '.exe' : ''}`,
    exeFile: `${name}${isWin ? '.exe' : ''}`,
    tmpFile: `${name}-${SIDECAR_HOST}.${urlExt}`,
    downloadURL: `https://github.com/${SERVICE_REPO}/releases/download/${version}/${name}-${SIDECAR_HOST}.${urlExt}`,
  }
}

async function resolveWintun(): Promise<void> {
  if (platform !== 'win32') return

  const wintunArchMap: Record<string, string> = {
    x64: 'amd64',
    ia32: 'x86',
    arm: 'arm',
    arm64: 'arm64',
  }
  const wintunArch = wintunArchMap[arch]
  if (!wintunArch) throw new Error(`unsupported arch ${arch}`)

  const url = 'https://www.wintun.net/builds/wintun-0.14.1.zip'
  const expectedHash =
    '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51'
  const tempDir = path.join(TEMP_DIR, 'wintun')
  const tempZip = path.join(tempDir, 'wintun.zip')
  const targetPath = path.join(TAURI_APP_DIR, 'resources', 'wintun.dll')

  if (!FORCE && (await exists(targetPath))) return

  await ensureDir(tempDir)

  if (!(await exists(tempZip))) {
    await downloadFile(url, tempZip)
  }

  // verify SHA-256
  const fileData = await Deno.readFile(tempZip)
  const hashBuffer = await crypto.subtle.digest('SHA-256', fileData)
  const hashHex = Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
  if (hashHex !== expectedHash) {
    throw new Error(`wintun hash not match ${hashHex}`)
  }

  // extract
  const zip = new AdmZip(tempZip)
  zip.extractAllTo(tempDir, true)

  // recursively find wintun.dll for the target arch
  function findDlls(dir: string): string[] {
    const results: string[] = []
    for (const entry of Deno.readDirSync(dir)) {
      const fullPath = path.join(dir, entry.name)
      if (entry.isDirectory) {
        results.push(...findDlls(fullPath))
      } else if (entry.name === 'wintun.dll' && fullPath.includes(wintunArch)) {
        results.push(fullPath)
      }
    }
    return results
  }

  const dlls = findDlls(tempDir)
  const dll = dlls[0]
  if (!dll) throw new Error(`wintun not found for arch ${wintunArch}`)

  await ensureDir(path.dirname(targetPath))
  await Deno.copyFile(dll, targetPath)
  await Deno.remove(tempDir, { recursive: true })

  consola.success(colorize`resolve {green wintun.dll} finished`)
}

// === Task Runner ===

const tasks: Array<{
  name: string
  func: () => Promise<void>
  retry: number
  winOnly?: boolean
}> = [
  {
    name: 'clash',
    func: () =>
      resolveSidecar(getClashBackupInfo(), {
        force: FORCE,
      }),
    retry: 5,
  },
  {
    name: 'mihomo',
    func: () => resolveSidecar(getClashMetaInfo(), { force: FORCE }),
    retry: 5,
  },
  {
    name: 'mihomo-alpha',
    func: () => resolveSidecar(getClashMetaAlphaInfo(), { force: FORCE }),
    retry: 5,
  },
  {
    name: 'clash-rs',
    func: () => resolveSidecar(getClashRustInfo(), { force: FORCE }),
    retry: 5,
  },
  {
    name: 'clash-rs-alpha',
    func: () => resolveSidecar(getClashRustAlphaInfo(), { force: FORCE }),
    retry: 5,
  },
  { name: 'wintun', func: () => resolveWintun(), retry: 5, winOnly: true },
  {
    name: 'nyanpasu-service',
    func: () => resolveSidecar(getNyanpasuServiceInfo(), { force: FORCE }),
    retry: 5,
  },
  {
    name: 'mmdb',
    func: () =>
      resolveResource(
        {
          file: 'Country.mmdb',
          downloadURL:
            'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb',
        },
        { force: FORCE },
      ),
    retry: 5,
  },
  {
    name: 'geoip',
    func: () =>
      resolveResource(
        {
          file: 'geoip.dat',
          downloadURL:
            'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat',
        },
        { force: FORCE },
      ),
    retry: 5,
  },
  {
    name: 'geosite',
    func: () =>
      resolveResource(
        {
          file: 'geosite.dat',
          downloadURL:
            'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat',
        },
        { force: FORCE },
      ),
    retry: 5,
  },
  {
    name: 'enableLoopback',
    func: () =>
      resolveResource(
        {
          file: 'enableLoopback.exe',
          downloadURL:
            'https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe',
        },
        { force: FORCE },
      ),
    retry: 5,
    winOnly: true,
  },
]

async function runTask(): Promise<void> {
  const task = tasks.shift()
  if (!task) return
  if (task.winOnly && platform !== 'win32') return runTask()

  for (let i = 0; i < task.retry; i++) {
    try {
      await task.func()
      break
    } catch (err) {
      consola.warn(`task::${task.name} try ${i} ==`, err)
      if (i === task.retry - 1) {
        consola.fatal(`task::${task.name} failed`, err)
        Deno.exit(1)
      }
    }
  }

  return runTask()
}

// === Main ===

consola.start('start check and download resources...')

const concurrency = Math.ceil(navigator.hardwareConcurrency / 2) || 2
const jobs = Array.from({ length: concurrency }, () => runTask())

await Promise.all(jobs)

console.log(figlet.textSync('Clash Nyanpasu', { whitespaceBreak: true }))
consola.success('all resources download finished\n')
consola.log('  next command:\n')
consola.log('    pnpm dev - development')
consola.log('    pnpm dev:diff - deadlock development (recommend)')
