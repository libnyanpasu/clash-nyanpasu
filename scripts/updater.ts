import fetch from 'node-fetch'
import { context, getOctokit } from '@actions/github'
import { resolveUpdateLog } from './updatelog'
import { getGithubUrl } from './utils'
import { colorize, consola } from './utils/logger'

const UPDATE_TAG_NAME = 'updater'
const UPDATE_JSON_FILE = 'update.json'
const UPDATE_JSON_PROXY = 'update-proxy.json'
const UPDATE_FIXED_WEBVIEW_FILE = 'update-fixed-webview.json'
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-fixed-webview-proxy.json'
const UPDATE_RELEASE_BODY = process.env.RELEASE_BODY || ''

const isFixedWebview = process.argv.includes('--fixed-webview')

/// generate update.json
/// upload to update tag's release asset
async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required')
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo }
  const github = getOctokit(process.env.GITHUB_TOKEN)

  const { data: tags } = await github.rest.repos.listTags({
    ...options,
    per_page: 10,
    page: 1,
  })

  // get the latest publish tag
  const tag = tags.find((t) => t.name.startsWith('v'))
  if (!tag) throw new Error('could not found the latest tag')
  consola.debug(colorize`latest tag: {gray.bold ${tag.name}}`)

  const { data: latestRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: tag.name,
  })

  let updateLog: string | null = null
  try {
    updateLog = await resolveUpdateLog(tag.name)
  } catch (err) {
    consola.error(err)
  }

  const updateData = {
    name: tag.name,
    notes: UPDATE_RELEASE_BODY || updateLog || latestRelease.body,
    pub_date: new Date().toISOString(),
    platforms: {
      win64: { signature: '', url: '' }, // compatible with older formats
      linux: { signature: '', url: '' }, // compatible with older formats
      darwin: { signature: '', url: '' }, // compatible with older formats
      'darwin-aarch64': { signature: '', url: '' },
      'darwin-intel': { signature: '', url: '' },
      'darwin-x86_64': { signature: '', url: '' },
      'linux-x86_64': { signature: '', url: '' },
      // "linux-aarch64": { signature: "", url: "" },
      // "linux-armv7": { signature: "", url: "" },
      'windows-x86_64': { signature: '', url: '' },
      'windows-i686': { signature: '', url: '' },
      'windows-aarch64': { signature: '', url: '' },
    },
  }

  const promises = latestRelease.assets.map(async (asset) => {
    const { name, browser_download_url: browserDownloadUrl } = asset

    function isMatch(name: string, extension: string, arch: string) {
      return (
        name.endsWith(extension) &&
        name.includes(arch) &&
        (isFixedWebview
          ? name.includes('fixed-webview')
          : !name.includes('fixed-webview'))
      )
    }

    // win64 url
    if (isMatch(name, '.nsis.zip', 'x64')) {
      updateData.platforms.win64.url = browserDownloadUrl
      updateData.platforms['windows-x86_64'].url = browserDownloadUrl
    }
    // win64 signature
    if (isMatch(name, '.nsis.zip.sig', 'x64')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms.win64.signature = sig
      updateData.platforms['windows-x86_64'].signature = sig
    }

    // win32 url
    if (isMatch(name, '.nsis.zip', 'x86')) {
      updateData.platforms['windows-i686'].url = browserDownloadUrl
    }
    // win32 signature
    if (isMatch(name, '.nsis.zip.sig', 'x86')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms['windows-i686'].signature = sig
    }

    // win arm64 url
    if (isMatch(name, '.nsis.zip', 'arm64')) {
      updateData.platforms['windows-aarch64'].url = browserDownloadUrl
    }
    // win arm64 signature
    if (isMatch(name, '.nsis.zip.sig', 'arm64')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms['windows-aarch64'].signature = sig
    }

    // darwin url (intel)
    if (name.endsWith('.app.tar.gz') && !name.includes('aarch')) {
      updateData.platforms.darwin.url = browserDownloadUrl
      updateData.platforms['darwin-intel'].url = browserDownloadUrl
      updateData.platforms['darwin-x86_64'].url = browserDownloadUrl
    }
    // darwin signature (intel)
    if (name.endsWith('.app.tar.gz.sig') && !name.includes('aarch')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms.darwin.signature = sig
      updateData.platforms['darwin-intel'].signature = sig
      updateData.platforms['darwin-x86_64'].signature = sig
    }

    // darwin url (aarch)
    if (name.endsWith('aarch64.app.tar.gz')) {
      updateData.platforms['darwin-aarch64'].url = browserDownloadUrl
    }
    // darwin signature (aarch)
    if (name.endsWith('aarch64.app.tar.gz.sig')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms['darwin-aarch64'].signature = sig
    }

    // linux url
    if (name.endsWith('.AppImage.tar.gz')) {
      updateData.platforms.linux.url = browserDownloadUrl
      updateData.platforms['linux-x86_64'].url = browserDownloadUrl
    }
    // linux signature
    if (name.endsWith('.AppImage.tar.gz.sig')) {
      const sig = await getSignature(browserDownloadUrl)
      updateData.platforms.linux.signature = sig
      updateData.platforms['linux-x86_64'].signature = sig
    }
  })

  await Promise.allSettled(promises)
  consola.info(updateData)

  // maybe should test the signature as well
  // delete the null field
  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      consola.error(`failed to parse release for "${key}"`)
      delete updateData.platforms[key as keyof typeof updateData.platforms]
    }
  })

  // 生成一个代理github的更新文件
  // 使用 https://hub.fastgit.xyz/ 做github资源的加速
  const updateDataNew = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData

  Object.entries(updateDataNew.platforms).forEach(([key, value]) => {
    if (value.url) {
      updateDataNew.platforms[key as keyof typeof updateData.platforms].url =
        getGithubUrl(value.url)
    } else {
      consola.error(`updateDataNew.platforms.${key} is null`)
    }
  })

  // update the update.json
  const { data: updateRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: UPDATE_TAG_NAME,
  })

  // delete the old assets
  for (const asset of updateRelease.assets) {
    if (
      isFixedWebview
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      })
    }

    if (
      isFixedWebview
        ? asset.name === UPDATE_FIXED_WEBVIEW_PROXY
        : asset.name === UPDATE_JSON_PROXY
    ) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err) => {
          consola.error(err)
        }) // do not break the pipeline
    }
  }

  // upload new assets
  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: isFixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE,
    data: JSON.stringify(updateData, null, 2),
  })

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: isFixedWebview ? UPDATE_FIXED_WEBVIEW_PROXY : UPDATE_JSON_PROXY,
    data: JSON.stringify(updateDataNew, null, 2),
  })
}

// get the signature file content
async function getSignature(url: string) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
  })

  return response.text()
}

resolveUpdater().catch((err) => {
  consola.error(err)
})
