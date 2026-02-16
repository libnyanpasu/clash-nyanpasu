import { retry } from 'jsr:@std/async@1/retry'
import { format as formatBytes } from 'jsr:@std/fmt@1/bytes'
import { ensureDir, exists } from 'jsr:@std/fs'
import * as path from 'jsr:@std/path'
import { TelegramClient } from 'npm:telegram'
import { StringSession } from 'npm:telegram/sessions'
import { consola } from './utils/logger.ts'

// --- env helpers ---

function requireEnv(name: string): string {
  const value = Deno.env.get(name)
  if (!value) {
    consola.fatal(`${name} is required`)
    Deno.exit(1)
  }
  return value
}

const nightlyBuild = Deno.args.includes('--nightly')

const TELEGRAM_TOKEN = requireEnv('TELEGRAM_TOKEN')
const TELEGRAM_TO = requireEnv('TELEGRAM_TO')
const TELEGRAM_TO_NIGHTLY = requireEnv('TELEGRAM_TO_NIGHTLY')
const GITHUB_TOKEN = requireEnv('GITHUB_TOKEN')
const TELEGRAM_API_ID = Number(requireEnv('TELEGRAM_API_ID'))
const TELEGRAM_API_HASH = requireEnv('TELEGRAM_API_HASH')
const FILE_SERVER_TOKEN = requireEnv('FILE_SERVER_TOKEN')
const WORKFLOW_RUN_ID = Deno.env.get('WORKFLOW_RUN_ID')

// --- constants ---

const FILE_SERVER_UPLOAD_URL = 'https://file-server.elaina.moe/upload'
const FILE_SERVER_BIN_URL = 'https://file-server.elaina.moe/bin'

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..')
const TEMP_DIR = path.join(WORKSPACE_ROOT, 'node_modules/.verge')

const repoInfo = { owner: 'libnyanpasu', repo: 'clash-nyanpasu' } as const

const resourceFormats = [
  '.exe',
  'portable.zip',
  '.rpm',
  '.deb',
  '.dmg',
  '.AppImage',
]

// --- helpers ---

function isValidFormat(fileName: string): boolean {
  return resourceFormats.some((fmt) => fileName.endsWith(fmt))
}

function getFileSize(filePath: string): string {
  const stat = Deno.statSync(filePath)
  return formatBytes(stat.size ?? 0)
}

function getGitShortHash(): string {
  const cmd = new Deno.Command('git', {
    args: ['rev-parse', '--short', 'HEAD'],
    stdout: 'piped',
  })
  const { stdout } = cmd.outputSync()
  return new TextDecoder().decode(stdout).trim()
}

async function getVersion(): Promise<string> {
  const pkgPath = path.join(WORKSPACE_ROOT, 'package.json')
  const pkg = JSON.parse(await Deno.readTextFile(pkgPath))
  return pkg.version as string
}

async function downloadFile(url: string, destPath: string): Promise<void> {
  consola.debug(`download "${url}" to "${destPath}"`)

  const response = await fetch(url, {
    method: 'GET',
    headers: {
      'Content-Type': 'application/octet-stream',
      'User-Agent':
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0',
    },
  })

  if (!response.ok) {
    throw new Error(`download failed: ${response.statusText}`)
  }

  const buffer = new Uint8Array(await response.arrayBuffer())
  await Deno.writeFile(destPath, buffer)

  consola.success(`download finished "${url.split('/').at(-1)}"`)
}

interface GitHubAsset {
  name: string
  browser_download_url: string
}

interface GitHubRelease {
  assets: GitHubAsset[]
}

async function fetchRelease(): Promise<GitHubRelease> {
  const { owner, repo } = repoInfo
  const url = nightlyBuild
    ? `https://api.github.com/repos/${owner}/${repo}/releases/tags/pre-release`
    : `https://api.github.com/repos/${owner}/${repo}/releases/latest`

  const resp = await fetch(url, {
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${GITHUB_TOKEN}`,
      'X-GitHub-Api-Version': '2022-11-28',
    },
  })

  if (!resp.ok) {
    throw new Error(`GitHub API error: ${resp.status} ${resp.statusText}`)
  }

  return (await resp.json()) as GitHubRelease
}

// --- file server upload ---

interface UploadResult {
  fileName: string
  downloadUrl: string
}

async function uploadToFileServer(filePath: string): Promise<UploadResult> {
  const fileName = path.basename(filePath)
  const fileData = await Deno.readFile(filePath)

  consola.info(
    `uploading ${fileName} (${formatBytes(fileData.byteLength)}) to file server...`,
  )

  const resp = await fetch(FILE_SERVER_UPLOAD_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/octet-stream',
      'x-file-name': fileName,
      Authorization: `Bearer ${FILE_SERVER_TOKEN}`,
    },
    body: fileData,
  })

  if (!resp.ok) {
    const body = await resp.text()
    throw new Error(
      `file server upload failed for ${fileName}: ${resp.status} ${resp.statusText} - ${body}`,
    )
  }

  const json = (await resp.json()) as { id: string }
  const downloadUrl = `${FILE_SERVER_BIN_URL}/${json.id}`

  consola.success(`uploaded ${fileName} -> ${downloadUrl}`)
  return { fileName, downloadUrl }
}

// --- platform grouping ---

interface PlatformGroup {
  label: string
  filter: (filePath: string) => boolean
}

const platformGroups: PlatformGroup[] = [
  {
    label: 'Windows',
    filter: (item) =>
      !item.includes('fixed-webview') &&
      (item.endsWith('.exe') || item.endsWith('portable.zip')),
  },
  {
    label: 'macOS',
    filter: (item) => item.endsWith('.dmg'),
  },
  {
    label: 'Linux',
    filter: (item) =>
      (item.endsWith('.rpm') ||
        item.endsWith('.deb') ||
        item.endsWith('.AppImage')) &&
      !item.includes('armel') &&
      !item.includes('armhf'),
  },
  {
    label: 'Linux (armv7)',
    filter: (item) =>
      (item.endsWith('.rpm') ||
        item.endsWith('.deb') ||
        item.endsWith('.AppImage')) &&
      (item.includes('armel') || item.includes('armhf')),
  },
]

// --- main ---

async function main() {
  const client = new TelegramClient(
    new StringSession(''),
    TELEGRAM_API_ID,
    TELEGRAM_API_HASH,
    { connectionRetries: 5 },
  )

  await client.start({ botAuthToken: TELEGRAM_TOKEN })

  const release = await fetchRelease()
  const GIT_SHORT_HASH = getGitShortHash()
  const version = await getVersion()

  const resourceMapping: string[] = []
  const downloadTasks: Promise<void>[] = []

  for (const asset of release.assets) {
    if (isValidFormat(asset.name)) {
      const dest = path.join(TEMP_DIR, asset.name)
      resourceMapping.push(dest)
      downloadTasks.push(
        retry(() => downloadFile(asset.browser_download_url, dest), {
          maxAttempts: 5,
        }),
      )
    }
  }

  try {
    await ensureDir(TEMP_DIR)
    await Promise.all(downloadTasks)
  } catch (error) {
    consola.error(error)
    throw new Error('Error during download tasks')
  }

  for (const item of resourceMapping) {
    consola.log(`found ${item}, size: ${getFileSize(item)}`, await exists(item))
  }

  // upload all files to file server
  consola.start('Uploading files to file server...')

  const uploadResults: UploadResult[] = []
  for (const filePath of resourceMapping) {
    const result = await retry(() => uploadToFileServer(filePath), {
      maxAttempts: 5,
    })
    uploadResults.push(result)
  }

  consola.success(`Uploaded ${uploadResults.length} files to file server`)

  // build message with grouped download links
  const lines: string[] = []

  if (!nightlyBuild) {
    lines.push(
      `**Clash Nyanpasu ${version} Released!**`,
      '',
      'GitHub Release:',
      `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v${version}`,
    )
  } else {
    lines.push(
      `**Clash Nyanpasu Nightly Build \`${GIT_SHORT_HASH}\`**`,
      '',
      '⚠️ Could be unstable, use at your own risk.',
    )
    if (WORKFLOW_RUN_ID) {
      lines.push(
        '',
        'GitHub Actions:',
        `https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/${WORKFLOW_RUN_ID}`,
      )
    }
  }

  lines.push('', '--- Download Links ---')

  for (const group of platformGroups) {
    const groupFiles = uploadResults.filter((r) => group.filter(r.fileName))
    if (groupFiles.length === 0) continue

    lines.push('', `**${group.label}:**`)
    for (const file of groupFiles) {
      lines.push(`- [${file.fileName}](${file.downloadUrl})`)
    }
  }

  const messageText = lines.join('\n')
  const chatId = nightlyBuild ? TELEGRAM_TO_NIGHTLY : TELEGRAM_TO

  await client.sendMessage(chatId, { message: messageText })
  consola.success('Sent telegram notification')

  await client.disconnect()
  Deno.exit()
}

main().catch((error) => {
  consola.fatal(error)
  Deno.exit(1)
})
