// import { Telegraf } from "telegraf";
import { existsSync } from 'fs'
import path from 'path'
import { fstat, mkdirp } from 'fs-extra'
import pRetry from 'p-retry'
import { getOctokit } from '@actions/github'
import { version } from '../package.json'
import { array2text, getFileSize } from './utils'
import { downloadFile } from './utils/download'
import { TEMP_DIR } from './utils/env'
import { consola } from './utils/logger'
import { GIT_SHORT_HASH } from './utils/shell'
import { client } from './utils/telegram'

const nightlyBuild = process.argv.includes('--nightly')

if (!process.env.TELEGRAM_TOKEN) {
  throw new Error('TELEGRAM_TOKEN is required')
}

const TELEGRAM_TOKEN = process.env.TELEGRAM_TOKEN

if (!process.env.TELEGRAM_TO) {
  throw new Error('TELEGRAM_TO is required')
}

const TELEGRAM_TO = process.env.TELEGRAM_TO

if (!process.env.TELEGRAM_TO_NIGHTLY) {
  throw new Error('TELEGRAM_TO_NIGHTLY is required')
}

const TELEGRAM_TO_NIGHTLY = process.env.TELEGRAM_TO_NIGHTLY

if (!process.env.GITHUB_TOKEN) {
  throw new Error('GITHUB_TOKEN is required')
}

const GITHUB_TOKEN = process.env.GITHUB_TOKEN

const WORKFLOW_RUN_ID = process.env.WORKFLOW_RUN_ID

const resourceFormats = [
  '.exe',
  'portable.zip',
  '.rpm',
  '.deb',
  '.dmg',
  '.AppImage',
]

const isValidFormat = (fileName: string): boolean => {
  return resourceFormats.some((format) => fileName.endsWith(format))
}

const repoInfo = {
  owner: 'libnyanpasu',
  repo: 'clash-nyanpasu',
}

;(async () => {
  await client.start({
    botAuthToken: TELEGRAM_TOKEN,
  })

  const github = getOctokit(GITHUB_TOKEN)

  const content = nightlyBuild
    ? await github.rest.repos.getReleaseByTag({
        ...repoInfo,
        tag: 'pre-release',
      })
    : await github.rest.repos.getLatestRelease(repoInfo)

  const downloadTasks: Promise<void>[] = []

  const resourceMapping: string[] = []

  content.data.assets.forEach((asset) => {
    if (isValidFormat(asset.name)) {
      const _path = path.join(TEMP_DIR, asset.name)

      resourceMapping.push(_path)

      downloadTasks.push(
        pRetry(() => downloadFile(asset.browser_download_url, _path), {
          retries: 5,
        }),
      )
    }
  })

  try {
    mkdirp(TEMP_DIR)

    await Promise.all(downloadTasks)
  } catch (error) {
    consola.error(error)
    throw new Error('Error during download or upload tasks')
  }

  resourceMapping.forEach((item) => {
    consola.log(`founded ${item}, size: ${getFileSize(item)}`, existsSync(item))
  })

  if (!nightlyBuild) {
    await client.sendMessage(TELEGRAM_TO, {
      message: array2text([
        `Clash Nyanpasu ${version} Released!`,
        '',
        'Check out on GitHub:',
        ` - https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v${version}`,
      ]),
    })
    consola.success('Send release message to telegram successfully')
  } else {
    await client.sendMessage(TELEGRAM_TO_NIGHTLY, {
      message: array2text([
        `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH} released!`,
        '',
        'Could be unstable, use at your own risk. Download at:',
        `- https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/${WORKFLOW_RUN_ID}`,
        '',
        'You could also waiting for the telegram bot to upload the binaries, although it may take a while or even fail.',
      ]),
    })
  }

  consola.start('Staring upload tasks (nightly)')

  // upload windows binary
  consola.info('starting upload windows related binary: here is the list:')
  let filteredFile = resourceMapping.filter(
    (item) =>
      !item.includes('fixed-webview') &&
      (item.endsWith('.exe') || item.endsWith('portable.zip')),
  )
  filteredFile.forEach((v) => {
    consola.debug(`file: ${v}, size:${getFileSize(v)}`)
  })
  await pRetry(
    () =>
      client.sendFile(TELEGRAM_TO_NIGHTLY, {
        file: filteredFile,
        forceDocument: true,
        caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH} for Windows`,
        workers: 16,
        progressCallback: (...args) => {
          console.log('progressCallback', args)
        },
      }),
    { retries: 5 },
  )

  consola.info('starting upload macos related binary: here is the list:')
  filteredFile = resourceMapping.filter((item) => item.endsWith('.dmg'))
  filteredFile.forEach((v) => {
    consola.debug(`file: ${v}, size:${getFileSize(v)}`)
  })

  // upload macOS binary
  await pRetry(
    () =>
      client.sendFile(TELEGRAM_TO_NIGHTLY, {
        file: filteredFile,
        forceDocument: true,
        caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH} for macOS`,
        workers: 16,
      }),
    { retries: 5 },
  )

  consola.info(
    'starting upload Linux related binary, part 1: here is the list:',
  )
  filteredFile = resourceMapping.filter(
    (item) =>
      (item.endsWith('.rpm') ||
        item.endsWith('.deb') ||
        item.endsWith('.AppImage')) &&
      !item.includes('armel') &&
      !item.includes('armhf'),
  )
  filteredFile.forEach((v) => {
    consola.debug(`file: ${v}, size:${getFileSize(v)}`)
  })

  // upload linux binary
  await pRetry(
    () =>
      client.sendFile(TELEGRAM_TO_NIGHTLY, {
        file: filteredFile,
        forceDocument: true,
        caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH} for Linux main target`,
        workers: 16,
      }),
    { retries: 5 },
  )

  consola.info(
    'starting upload Linux related binary, part 2: here is the list:',
  )
  filteredFile = resourceMapping.filter(
    (item) =>
      ((item.endsWith('.rpm') ||
        item.endsWith('.deb') ||
        item.endsWith('.AppImage')) &&
        item.includes('armel')) ||
      item.includes('armhf'),
  )
  filteredFile.forEach((v) => {
    consola.debug(`file: ${v}, size:${getFileSize(v)}`)
  })

  // upload linux binary
  await pRetry(
    () =>
      client.sendFile(TELEGRAM_TO_NIGHTLY, {
        file: filteredFile,
        forceDocument: true,
        caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH} for Linux armv7 target`,
        workers: 16,
      }),
    { retries: 5 },
  )

  consola.success('Upload finished (nightly)')

  await client.disconnect()

  process.exit()
})().catch((error) => {
  consola.fatal(error)
  process.exit(1)
})
