import path from 'node:path'
import AdmZip from 'adm-zip'
import fs from 'fs-extra'
import { context, getOctokit } from '@actions/github'
import packageJson from '../package.json'
import { TAURI_APP_DIR } from './utils/env'
import { colorize, consola } from './utils/logger'

const RUST_ARCH = process.env.RUST_ARCH || 'x86_64'
const fixedWebview = process.argv.includes('--fixed-webview')

/// Script for ci
/// 打包绿色版/便携版 (only Windows)
async function resolvePortable() {
  if (process.platform !== 'win32') return

  const buildDir = path.join(
    RUST_ARCH === 'x86_64'
      ? 'backend/target/release'
      : `backend/target/${RUST_ARCH}-pc-windows-msvc/release`,
  )

  const configDir = path.join(buildDir, '.config')

  if (!(await fs.pathExists(buildDir))) {
    throw new Error('could not found the release dir')
  }

  await fs.ensureDir(configDir)
  await fs.createFile(path.join(configDir, 'PORTABLE'))

  const zip = new AdmZip()
  let mainEntryPath = path.join(buildDir, 'Clash Nyanpasu.exe')
  if (!(await fs.pathExists(mainEntryPath))) {
    mainEntryPath = path.join(buildDir, 'clash-nyanpasu.exe')
  }
  zip.addLocalFile(mainEntryPath)
  zip.addLocalFile(path.join(buildDir, 'clash.exe'))
  zip.addLocalFile(path.join(buildDir, 'mihomo.exe'))
  zip.addLocalFile(path.join(buildDir, 'mihomo-alpha.exe'))
  zip.addLocalFile(path.join(buildDir, 'nyanpasu-service.exe'))
  zip.addLocalFile(path.join(buildDir, 'clash-rs.exe'))
  zip.addLocalFile(path.join(buildDir, 'clash-rs-alpha.exe'))
  zip.addLocalFolder(path.join(buildDir, 'resources'), 'resources')

  if (fixedWebview) {
    const webviewPath = (await fs.readdir(TAURI_APP_DIR)).find((file) =>
      file.includes('WebView2'),
    )
    if (!webviewPath) {
      throw new Error('WebView2 runtime not found')
    }
    zip.addLocalFolder(
      path.join(TAURI_APP_DIR, webviewPath),
      path.basename(webviewPath),
    )
  }

  zip.addLocalFolder(configDir, '.config')

  const { version } = packageJson

  const zipFile = `Clash.Nyanpasu_${version}_${RUST_ARCH}${fixedWebview ? '_fixed-webview' : ''}_portable.zip`
  zip.writeZip(zipFile)

  consola.success('create portable zip successfully')

  // push release assets
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required')
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo }
  const github = getOctokit(process.env.GITHUB_TOKEN)

  consola.info('upload to ', process.env.TAG_NAME || `v${version}`)

  const { data: release } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: process.env.TAG_NAME || `v${version}`,
  })

  consola.debug(colorize`releaseName: {green ${release.name}}`)

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: release.id,
    name: zipFile,
    // @ts-expect-error data is Buffer should work fine
    data: zip.toBuffer(),
  })
}

resolvePortable().catch((err) => {
  consola.error(err)
  process.exit(1)
})
