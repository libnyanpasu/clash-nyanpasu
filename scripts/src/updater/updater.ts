import { mkdir, writeFile } from 'node:fs/promises'
import * as path from 'node:path'
import { parseArgs } from 'node:util'
import { Octokit } from 'octokit'
import { colorize, consola } from '../shared/logger.ts'
import { selectChannelRelease, type ReleaseChannel } from './release-channel.ts'
import {
  attachSourceforgeMirrors,
  canonicalSourceforgeMirrorManifest,
  mapUpdaterPlatformUrls,
  parseSourceforgeMirrorManifest,
  SOURCEFORGE_MIRRORS_ASSET_NAME,
  sourceforgeMirrorAssetsEqual,
  type SourceforgeMirrorManifest,
} from './sourceforge-mirrors.ts'
import { resolveUpdateLog } from './updatelog.ts'
import {
  collectUpdaterPlatforms,
  UPDATER_TARGETS,
} from './updater-platforms.ts'

const GITHUB_PROXY = 'https://nyanpasu-script.majokeiko.com/'
const UPDATE_TAG_NAME = 'updater'
const UPDATE_RELEASE_BODY = process.env['RELEASE_BODY'] ?? ''

const { values: argv } = parseArgs({
  options: { 'cache-path': { type: 'string' } },
  strict: false,
})

function getGithubUrl(url: string): string {
  // The proxy expects the GitHub path without the github.com host, e.g.
  // https://nyanpasu-script.majokeiko.com/<owner>/<repo>/releases/download/...
  return new URL(
    url.replace(/^https?:\/\/github\.com\//, ''),
    GITHUB_PROXY,
  ).toString()
}

function getRepoContext() {
  const token = process.env['GITHUB_TOKEN']
  if (!token) throw new Error('GITHUB_TOKEN is required')
  const repoStr = process.env['GITHUB_REPOSITORY'] ?? ''
  const [owner, repo] = repoStr.split('/')
  if (!owner || !repo) throw new Error('GITHUB_REPOSITORY must be owner/repo')
  return { token, owner, repo }
}

async function getSignature(url: string) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
    signal: AbortSignal.timeout(30_000),
  })
  if (!response.ok) {
    await response.body?.cancel()
    throw new Error(`Failed to fetch signature: HTTP ${response.status}`)
  }
  return response.text()
}

async function loadSourceforgeMirrors(
  assets: readonly { name: string; browser_download_url: string }[],
): Promise<SourceforgeMirrorManifest | undefined> {
  const matches = assets.filter(
    (asset) => asset.name === SOURCEFORGE_MIRRORS_ASSET_NAME,
  )
  if (matches.length > 1) {
    throw new Error('Release has multiple SourceForge mirror manifests')
  }
  const asset = matches[0]
  if (!asset) return undefined
  const response = await fetch(asset.browser_download_url, {
    signal: AbortSignal.timeout(30_000),
  })
  if (!response.ok) {
    await response.body?.cancel()
    throw new Error(
      `Failed to fetch SourceForge release metadata: HTTP ${response.status}`,
    )
  }
  return parseSourceforgeMirrorManifest(await response.text())
}

async function persistSourceforgeMirrors(
  github: Octokit,
  options: { owner: string; repo: string },
  manifest: SourceforgeMirrorManifest,
): Promise<SourceforgeMirrorManifest> {
  if (manifest.channel !== 'release') {
    throw new Error(
      'Stable and beta updater generation requires release SourceForge metadata',
    )
  }
  const { data: release } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: manifest.buildId,
  })
  if (release.tag_name !== manifest.buildId) {
    throw new Error(
      'SourceForge metadata tag does not match the GitHub release',
    )
  }
  const { data: assets } = await github.rest.repos.listReleaseAssets({
    ...options,
    release_id: release.id,
    per_page: 100,
  })
  const platforms = await collectUpdaterPlatforms(
    assets,
    getSignature,
    manifest.buildId,
  )
  attachSourceforgeMirrors(platforms, assets, manifest, manifest.buildId)
  const existing = assets.filter(
    (asset) => asset.name === SOURCEFORGE_MIRRORS_ASSET_NAME,
  )
  if (existing.length > 1) {
    throw new Error('Release has multiple SourceForge mirror manifests')
  }
  const canonical = canonicalSourceforgeMirrorManifest(manifest)
  if (existing[0]) {
    const priorResponse = await fetch(existing[0].browser_download_url, {
      signal: AbortSignal.timeout(30_000),
    })
    if (!priorResponse.ok) {
      await priorResponse.body?.cancel()
      throw new Error(
        `Failed to read existing SourceForge metadata: HTTP ${priorResponse.status}`,
      )
    }
    const prior = parseSourceforgeMirrorManifest(await priorResponse.text())
    if (!sourceforgeMirrorAssetsEqual(prior, manifest)) {
      throw new Error(
        `SourceForge mirror metadata is immutable for release ${manifest.buildId}`,
      )
    }
    return prior
  }
  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: release.id,
    name: SOURCEFORGE_MIRRORS_ASSET_NAME,
    data: canonical,
  })
  return manifest
}

async function saveToCache(fileName: string, content: string) {
  const cachePath = argv['cache-path']
  if (typeof cachePath !== 'string' || !cachePath) return
  try {
    await mkdir(cachePath, { recursive: true })
    const filePath = path.join(cachePath, fileName)
    await writeFile(filePath, content)
    consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`)
  } catch (err) {
    consola.error(`Failed to save cache file: ${err}`)
  }
}

async function resolveUpdater(channel: ReleaseChannel) {
  const suffix = channel === 'beta' ? '-beta' : ''
  const UPDATE_JSON_FILE = `update${suffix}.json`
  const UPDATE_JSON_PROXY = `update${suffix}-proxy.json`
  const { token, owner, repo } = getRepoContext()
  const github = new Octokit({ auth: token })
  const options = { owner, repo }
  const incomingMirrorJson = process.env['SOURCEFORGE_MIRRORS_JSON']?.trim()
  const incomingMirrors = incomingMirrorJson
    ? parseSourceforgeMirrorManifest(incomingMirrorJson)
    : undefined
  const persistedIncomingMirrors = incomingMirrors
    ? await persistSourceforgeMirrors(github, options, incomingMirrors)
    : undefined

  const releases = await github.paginate(github.rest.repos.listReleases, {
    ...options,
    per_page: 100,
  })
  const latestRelease = selectChannelRelease(releases, channel)
  if (!latestRelease) throw new Error(`No release found for ${channel}`)
  const tag = latestRelease.tag_name
  consola.debug(`${channel} release: ${tag}`)

  let updateLog: string | null = null
  try {
    updateLog = await resolveUpdateLog(tag)
  } catch (err) {
    consola.error(err)
  }

  const collectedPlatforms = await collectUpdaterPlatforms(
    latestRelease.assets,
    getSignature,
    tag,
  )
  const releaseMirrors =
    persistedIncomingMirrors?.buildId === tag
      ? persistedIncomingMirrors
      : await loadSourceforgeMirrors(latestRelease.assets)
  const updateData = {
    name: tag,
    notes:
      (process.env['RELEASE_TAG'] === tag && UPDATE_RELEASE_BODY) ||
      updateLog ||
      latestRelease.body,
    pub_date: releaseMirrors?.publishedAt ?? new Date().toISOString(),
    platforms: attachSourceforgeMirrors(
      collectedPlatforms,
      latestRelease.assets,
      releaseMirrors,
      tag,
    ),
  }

  consola.info(updateData)
  for (const target of UPDATER_TARGETS) {
    if (!updateData.platforms[target]) {
      consola.error(`failed to parse release for "${target}"`)
    }
  }

  const updateDataNew = {
    ...updateData,
    platforms: mapUpdaterPlatformUrls(updateData.platforms, getGithubUrl),
  }

  const { data: updateRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: UPDATE_TAG_NAME,
  })

  for (const asset of updateRelease.assets as Array<{
    name: string
    id: number
  }>) {
    if (asset.name === UPDATE_JSON_FILE) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      })
    }
    if (asset.name === UPDATE_JSON_PROXY) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err: unknown) => {
          consola.error(err)
        })
    }
  }

  const mainContent = JSON.stringify(updateData, null, 2)
  const proxyContent = JSON.stringify(updateDataNew, null, 2)

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_FILE,
    data: mainContent,
  })
  await saveToCache(UPDATE_JSON_FILE, mainContent)

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_PROXY,
    data: proxyContent,
  })
  await saveToCache(UPDATE_JSON_PROXY, proxyContent)
}

async function main() {
  await resolveUpdater('stable')
  await resolveUpdater('beta')
}

main().catch((err) => {
  consola.error(err)
  process.exit(1)
})
