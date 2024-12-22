/**
 * Build and upload assets
 * for macOS(aarch)
 */
import path from 'node:path'
import fs from 'fs-extra'
import { context, getOctokit } from '@actions/github'
import pkgJson from '../package.json'
import { consola } from './utils/logger'

async function resolve() {
  if (!process.env.GITHUB_TOKEN) {
    throw new Error('GITHUB_TOKEN is required')
  }
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
    throw new Error('TAURI_SIGNING_PRIVATE_KEY is required')
  }
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    throw new Error('TAURI_SIGNING_PRIVATE_KEY_PASSWORD is required')
  }

  const { version } = pkgJson

  const tag = process.env.TAG_NAME || `v${version}`

  consola.info(`Upload to tag ${tag}`)

  const cwd = process.cwd()
  const bundlePath = path.join(
    'backend/target/aarch64-apple-darwin/release/bundle',
  )
  const join = (p: string) => path.join(bundlePath, p)

  const appPathList = [
    join('macos/Clash Nyanpasu.aarch64.app.tar.gz'),
    join('macos/Clash Nyanpasu.aarch64.app.tar.gz.sig'),
  ]

  for (const appPath of appPathList) {
    if (fs.pathExistsSync(appPath)) {
      fs.removeSync(appPath)
    }
  }

  fs.copyFileSync(join('macos/Clash Nyanpasu.app.tar.gz'), appPathList[0])
  fs.copyFileSync(join('macos/Clash Nyanpasu.app.tar.gz.sig'), appPathList[1])

  const options = { owner: context.repo.owner, repo: context.repo.repo }
  const github = getOctokit(process.env.GITHUB_TOKEN)

  const { data: release } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag,
  })

  if (!release.id) throw new Error('failed to find the release')

  await uploadAssets(release.id, [
    join(`dmg/Clash Nyanpasu_${version}_aarch64.dmg`),
    ...appPathList,
  ])
}

// From tauri-apps/tauri-action
// https://github.com/tauri-apps/tauri-action/blob/dev/packages/action/src/upload-release-assets.ts
async function uploadAssets(releaseId: number, assets: string[]) {
  const GITHUB_TOKEN = process.env.GITHUB_TOKEN
  if (!GITHUB_TOKEN) {
    throw new Error('GITHUB_TOKEN is required')
  }
  const github = getOctokit(GITHUB_TOKEN)

  // Determine content-length for header to upload asset
  const contentLength = (filePath: string) => fs.statSync(filePath).size

  for (const assetPath of assets) {
    const headers = {
      'content-type': 'application/zip',
      'content-length': contentLength(assetPath),
    }

    const ext = path.extname(assetPath)
    const filename = path.basename(assetPath).replace(ext, '')
    const assetName = path.dirname(assetPath).includes(`target${path.sep}debug`)
      ? `${filename}-debug${ext}`
      : `${filename}${ext}`

    consola.start(`Uploading ${assetName}...`)

    try {
      await github.rest.repos.uploadReleaseAsset({
        headers,
        name: assetName,
        // https://github.com/tauri-apps/tauri-action/pull/45
        // @ts-expect-error error TS2322: Type 'Buffer' is not assignable to type 'string'.
        data: fs.readFileSync(assetPath),
        owner: context.repo.owner,
        repo: context.repo.repo,
        release_id: releaseId,
      })
      consola.success(`Uploaded ${assetName}`)
    } catch (error) {
      consola.error(
        'Failed to upload release asset',
        error instanceof Error ? error.message : error,
      )
    }
  }
}

resolve()
