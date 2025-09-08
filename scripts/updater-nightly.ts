import { execSync } from 'child_process'
import fs from 'fs/promises'
import path from 'path'
import { camelCase, upperFirst } from 'lodash-es'
import semver from 'semver'
import { fetch } from 'undici'
import yargs from 'yargs'
import { hideBin } from 'yargs/helpers'
import { z } from 'zod'
import { context, getOctokit } from '@actions/github'
import tauriNightly from '../backend/tauri/overrides/nightly.conf.json'
import { getGithubUrl, getProxyAgent } from './utils'
import { colorize, consola } from './utils/logger'

const UPDATE_TAG_NAME = 'updater'
const UPDATE_JSON_FILE = 'update-nightly.json'
const UPDATE_JSON_PROXY = 'update-nightly-proxy.json'
const UPDATE_FIXED_WEBVIEW_FILE = 'update-nightly-fixed-webview.json'
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-nightly-fixed-webview-proxy.json'

const argv = yargs(hideBin(process.argv))
  .option('fixed-webview', {
    type: 'boolean',
    default: false,
  })
  .option('cache-path', {
    type: 'string',
    requiresArg: false,
  })
  .help()
  .parseSync()

/// generate update.json
/// upload to update tag's release asset
async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required')
  }
  consola.start('start to generate updater files')
  const options = {
    owner: context.repo.owner,
    repo: context.repo.repo,
  }
  const github = getOctokit(process.env.GITHUB_TOKEN)

  consola.debug('resolve latest pre-release files...')
  // latest pre-release tag
  const { data: latestPreRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: 'pre-release',
  })
  let shortHash = ''
  const latestContent = latestPreRelease.assets.find(
    (o) => o.name === 'latest.json',
  )
  // trying to get the short hash from the latest.json
  if (latestContent) {
    const schema = z.object({
      version: z.string().min(1),
    })
    const latest = schema.parse(
      await fetch(latestContent.browser_download_url, {
        dispatcher: getProxyAgent(),
      }).then((res) => res.json()),
    )

    const version = semver.parse(latest.version)
    if (version && version.build.length > 0) {
      console.log(version)
      shortHash = version.build[0]
    }
  }

  if (!shortHash) {
    shortHash = await execSync(`git rev-parse --short pre-release`)
      .toString()
      .replace('\n', '')
      .replace('\r', '')
      .slice(0, 7)
  }

  consola.info(`latest pre-release short hash: ${shortHash}`)
  const updateData = {
    name: `v${tauriNightly.version}-alpha+${shortHash}`,
    notes: 'Nightly build. Full changes see commit history.',
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

  const promises = latestPreRelease.assets.map(async (asset) => {
    const { name, browser_download_url: browserDownloadUrl } = asset

    function isMatch(name: string, extension: string, arch: string) {
      return (
        name.endsWith(extension) &&
        name.includes(arch) &&
        (argv.fixedWebview
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

  consola.debug('generate updater metadata...')
  // maybe should test the signature as well
  // delete the null field
  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      throw new Error(`failed to parse release for "${key}"`)
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
  consola.debug('update updater files...')
  let updateRelease
  try {
    const { data } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: UPDATE_TAG_NAME,
    })
    updateRelease = data
  } catch (err) {
    consola.error(err)
    consola.error('failed to get release by tag, create one')
    const { data } = await github.rest.repos.createRelease({
      ...options,
      tag_name: UPDATE_TAG_NAME,
      name: upperFirst(camelCase(UPDATE_TAG_NAME)),
      body: 'files for programs to check for updates',
      prerelease: true,
    })
    updateRelease = data
  }

  // delete the old assets
  for (const asset of updateRelease.assets) {
    if (
      argv.fixedWebview
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      })
    }

    if (
      argv.fixedWebview
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
    name: argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE,
    data: JSON.stringify(updateData, null, 2),
  })

  // cache the files if cache path is provided
  await saveToCache(
    argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE,
    JSON.stringify(updateData, null, 2),
  )

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_PROXY : UPDATE_JSON_PROXY,
    data: JSON.stringify(updateDataNew, null, 2),
  })

  // cache the proxy file if cache path is provided
  await saveToCache(
    argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_PROXY : UPDATE_JSON_PROXY,
    JSON.stringify(updateDataNew, null, 2),
  )

  consola.success('updater files updated')
}

async function saveToCache(fileName: string, content: string) {
  if (!argv.cachePath) return
  try {
    await fs.mkdir(argv.cachePath, { recursive: true })
    const filePath = path.join(argv.cachePath, fileName)
    await fs.writeFile(filePath, content, 'utf-8')
    consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`)
  } catch (err) {
    throw new Error(`Failed to save cache file: ${err}`)
  }
}

// get the signature file content
async function getSignature(url: string) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
    dispatcher: getProxyAgent(),
  })

  return response.text()
}

resolveUpdater().catch((err) => {
  consola.fatal(err)
  process.exit(1)
})
