import * as path from 'jsr:@std/path'
import { parseArgs } from 'jsr:@std/cli/parse-args'
import { parse as parseSemver } from 'jsr:@std/semver'
import { z } from 'npm:zod'
import { consola } from './utils/logger.ts'

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..')
const GITHUB_PROXY = 'https://gh-proxy.com/'

const GITHUB_TOKEN = Deno.env.get('GITHUB_TOKEN') || Deno.env.get('GH_TOKEN')

if (!GITHUB_TOKEN) {
  consola.fatal('GITHUB_TOKEN is not set')
  Deno.exit(1)
}

const GITHUB_REPOSITORY = Deno.env.get('GITHUB_REPOSITORY') || ''
const [owner, repo] = GITHUB_REPOSITORY.split('/')
if (!owner || !repo) {
  consola.fatal('GITHUB_REPOSITORY is not set or invalid (expected "owner/repo")')
  Deno.exit(1)
}

const args = parseArgs(Deno.args, {
  boolean: ['fixed-webview'],
  string: ['cache-path'],
  default: { 'fixed-webview': false },
})

const UPDATE_TAG_NAME = 'updater'
const UPDATE_JSON_FILE = 'update-nightly.json'
const UPDATE_JSON_PROXY = 'update-nightly-proxy.json'
const UPDATE_FIXED_WEBVIEW_FILE = 'update-nightly-fixed-webview.json'
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-nightly-fixed-webview-proxy.json'

const BASE_HEADERS: Record<string, string> = {
  Authorization: `Bearer ${GITHUB_TOKEN}`,
  Accept: 'application/vnd.github.v3+json',
  'X-GitHub-Api-Version': '2022-11-28',
}

async function githubFetch(
  endpoint: string,
  options: RequestInit = {},
): Promise<Response> {
  const url = `https://api.github.com${endpoint}`
  const res = await fetch(url, {
    ...options,
    headers: { ...BASE_HEADERS, ...(options.headers as Record<string, string> ?? {}) },
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`GitHub API ${res.status} ${url}: ${text}`)
  }
  return res
}

function getGithubUrl(url: string): string {
  return new URL(url.replace(/^https?:\/\//g, ''), GITHUB_PROXY).toString()
}

function isMatch(
  name: string,
  extension: string,
  arch: string,
  fixedWebview: boolean,
): boolean {
  return (
    name.endsWith(extension) &&
    name.includes(arch) &&
    (fixedWebview
      ? name.includes('fixed-webview')
      : !name.includes('fixed-webview'))
  )
}

async function getSignature(url: string): Promise<string> {
  const res = await fetch(url)
  return res.text()
}

async function run(cmd: string[]): Promise<string> {
  const p = new Deno.Command(cmd[0], { args: cmd.slice(1), stdout: 'piped' })
  const { stdout } = await p.output()
  return new TextDecoder().decode(stdout).trim()
}

async function uploadReleaseAsset(
  releaseId: number,
  name: string,
  content: string,
): Promise<void> {
  const encoded = new TextEncoder().encode(content)
  const res = await fetch(
    `https://uploads.github.com/repos/${owner}/${repo}/releases/${releaseId}/assets?name=${encodeURIComponent(name)}`,
    {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${GITHUB_TOKEN}`,
        'Content-Type': 'application/json',
        'Content-Length': String(encoded.length),
      },
      body: encoded,
    },
  )
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`Failed to upload asset "${name}": ${res.status} ${text}`)
  }
  consola.success(`Uploaded ${name}`)
}

async function saveToCache(fileName: string, content: string): Promise<void> {
  const cachePath = args['cache-path']
  if (!cachePath) return
  try {
    await Deno.mkdir(cachePath, { recursive: true })
    const filePath = path.join(cachePath, fileName)
    await Deno.writeTextFile(filePath, content)
    consola.success(`Cached file saved to: ${filePath}`)
  } catch (err) {
    consola.error(`Failed to save cache file: ${err}`)
  }
}

async function resolveUpdater(): Promise<void> {
  consola.start('Generating nightly updater files')

  // Read nightly version from tauri config
  const nightlyConfPath = path.join(
    WORKSPACE_ROOT,
    'backend/tauri/overrides/nightly.conf.json',
  )
  const nightlyConf = JSON.parse(await Deno.readTextFile(nightlyConfPath)) as {
    version: string
  }

  // Get pre-release info
  const preReleaseRes = await githubFetch(
    `/repos/${owner}/${repo}/releases/tags/pre-release`,
  )
  const preRelease = (await preReleaseRes.json()) as {
    id: number
    assets: Array<{ name: string; browser_download_url: string }>
  }

  // Try to get short hash from latest.json asset
  let shortHash = ''
  const latestJsonAsset = preRelease.assets.find((o) => o.name === 'latest.json')
  if (latestJsonAsset) {
    try {
      const schema = z.object({ version: z.string().min(1) })
      const latest = schema.parse(
        await fetch(latestJsonAsset.browser_download_url).then((r) => r.json()),
      )
      const version = parseSemver(latest.version)
      if (version && version.build.length > 0) {
        shortHash = version.build[0]
      }
    } catch (err) {
      consola.error(`Failed to parse latest.json: ${err}`)
    }
  }

  if (!shortHash) {
    shortHash = (
      await run(['git', 'rev-parse', '--short', 'pre-release'])
    ).slice(0, 7)
  }

  consola.info(`Latest pre-release short hash: ${shortHash}`)

  const updateData = {
    name: `v${nightlyConf.version}-alpha+${shortHash}`,
    notes: 'Nightly build. Full changes see commit history.',
    pub_date: new Date().toISOString(),
    platforms: {
      win64: { signature: '', url: '' },
      linux: { signature: '', url: '' },
      darwin: { signature: '', url: '' },
      'darwin-aarch64': { signature: '', url: '' },
      'darwin-intel': { signature: '', url: '' },
      'darwin-x86_64': { signature: '', url: '' },
      'linux-x86_64': { signature: '', url: '' },
      'windows-x86_64': { signature: '', url: '' },
      'windows-i686': { signature: '', url: '' },
      'windows-aarch64': { signature: '', url: '' },
    } as Record<string, { signature: string; url: string }>,
  }

  const fixedWebview = args['fixed-webview']

  await Promise.allSettled(
    preRelease.assets.map(async (asset) => {
      const { name, browser_download_url: url } = asset

      if (isMatch(name, '.nsis.zip', 'x64', fixedWebview)) {
        updateData.platforms['win64'].url = url
        updateData.platforms['windows-x86_64'].url = url
      }
      if (isMatch(name, '.nsis.zip.sig', 'x64', fixedWebview)) {
        const sig = await getSignature(url)
        updateData.platforms['win64'].signature = sig
        updateData.platforms['windows-x86_64'].signature = sig
      }
      if (isMatch(name, '.nsis.zip', 'x86', fixedWebview)) {
        updateData.platforms['windows-i686'].url = url
      }
      if (isMatch(name, '.nsis.zip.sig', 'x86', fixedWebview)) {
        updateData.platforms['windows-i686'].signature = await getSignature(url)
      }
      if (isMatch(name, '.nsis.zip', 'arm64', fixedWebview)) {
        updateData.platforms['windows-aarch64'].url = url
      }
      if (isMatch(name, '.nsis.zip.sig', 'arm64', fixedWebview)) {
        updateData.platforms['windows-aarch64'].signature =
          await getSignature(url)
      }
      if (name.endsWith('.app.tar.gz') && !name.includes('aarch')) {
        updateData.platforms['darwin'].url = url
        updateData.platforms['darwin-intel'].url = url
        updateData.platforms['darwin-x86_64'].url = url
      }
      if (name.endsWith('.app.tar.gz.sig') && !name.includes('aarch')) {
        const sig = await getSignature(url)
        updateData.platforms['darwin'].signature = sig
        updateData.platforms['darwin-intel'].signature = sig
        updateData.platforms['darwin-x86_64'].signature = sig
      }
      if (name.endsWith('aarch64.app.tar.gz')) {
        updateData.platforms['darwin-aarch64'].url = url
      }
      if (name.endsWith('aarch64.app.tar.gz.sig')) {
        updateData.platforms['darwin-aarch64'].signature =
          await getSignature(url)
      }
      if (name.endsWith('.AppImage.tar.gz')) {
        updateData.platforms['linux'].url = url
        updateData.platforms['linux-x86_64'].url = url
      }
      if (name.endsWith('.AppImage.tar.gz.sig')) {
        const sig = await getSignature(url)
        updateData.platforms['linux'].signature = sig
        updateData.platforms['linux-x86_64'].signature = sig
      }
    }),
  )

  consola.info(updateData)

  // Validate all platforms have URLs
  for (const [key, value] of Object.entries(updateData.platforms)) {
    if (!value.url) {
      throw new Error(`failed to parse release for "${key}"`)
    }
  }

  // Build proxy variant
  const updateDataProxy = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData
  for (const [key, value] of Object.entries(updateDataProxy.platforms)) {
    if (value.url) {
      updateDataProxy.platforms[key].url = getGithubUrl(value.url)
    }
  }

  // Get or create the updater release
  let updaterReleaseId: number
  let updaterAssets: Array<{ id: number; name: string }>

  try {
    const res = await githubFetch(
      `/repos/${owner}/${repo}/releases/tags/${UPDATE_TAG_NAME}`,
    )
    const data = (await res.json()) as {
      id: number
      assets: Array<{ id: number; name: string }>
    }
    updaterReleaseId = data.id
    updaterAssets = data.assets
  } catch {
    consola.error('Failed to get updater release, creating one')
    const res = await githubFetch(`/repos/${owner}/${repo}/releases`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        tag_name: UPDATE_TAG_NAME,
        name: 'Updater',
        body: 'Files for programs to check for updates',
        prerelease: true,
      }),
    })
    const data = (await res.json()) as {
      id: number
      assets: Array<{ id: number; name: string }>
    }
    updaterReleaseId = data.id
    updaterAssets = data.assets
  }

  const mainFile = fixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE
  const proxyFile = fixedWebview
    ? UPDATE_FIXED_WEBVIEW_PROXY
    : UPDATE_JSON_PROXY

  // Delete old assets
  for (const asset of updaterAssets) {
    if (asset.name === mainFile || asset.name === proxyFile) {
      await githubFetch(
        `/repos/${owner}/${repo}/releases/assets/${asset.id}`,
        { method: 'DELETE' },
      ).catch((err) => consola.error(`Failed to delete asset: ${err}`))
    }
  }

  const mainContent = JSON.stringify(updateData, null, 2)
  const proxyContent = JSON.stringify(updateDataProxy, null, 2)

  await uploadReleaseAsset(updaterReleaseId, mainFile, mainContent)
  await saveToCache(mainFile, mainContent)
  await uploadReleaseAsset(updaterReleaseId, proxyFile, proxyContent)
  await saveToCache(proxyFile, proxyContent)

  consola.success('Nightly updater files updated successfully')
}

resolveUpdater().catch((err) => {
  consola.fatal(err)
  Deno.exit(1)
})
