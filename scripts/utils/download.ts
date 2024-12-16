import { execSync } from 'child_process'
import path from 'path'
import zlib from 'zlib'
import AdmZip from 'adm-zip'
import fs from 'fs-extra'
import fetch, { type RequestInit } from 'node-fetch'
import * as tar from 'tar'
import { BinInfo } from 'types'
import { getProxyAgent } from './'
import { TAURI_APP_DIR, TEMP_DIR } from './env'
import { colorize, consola } from './logger'

/**
 * download sidecar and rename
 */
export const downloadFile = async (url: string, path: string) => {
  const options: Partial<RequestInit> = {}

  const httpProxy = getProxyAgent()

  if (httpProxy) {
    options.agent = httpProxy
  }

  const response = await fetch(url, {
    ...options,
    method: 'GET',
    headers: {
      'Content-Type': 'application/octet-stream',
      'User-Agent':
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0',
    },
  })

  const buffer = await response.arrayBuffer()

  await fs.writeFile(path, new Uint8Array(buffer))

  consola.success(colorize`download finished {gray "${url.split('/').at(-1)}"}`)
}

export const resolveSidecar = async (
  binInfo: PromiseLike<BinInfo> | BinInfo,
  platform: string,
  option?: { force?: boolean },
) => {
  const { name, targetFile, tmpFile, exeFile, downloadURL } = await binInfo

  consola.debug(colorize`resolve {cyan ${name}}...`)

  const sidecarDir = path.join(TAURI_APP_DIR, 'sidecar')

  const sidecarPath = path.join(sidecarDir, targetFile)

  await fs.mkdirp(sidecarDir)

  if (!option?.force && (await fs.pathExists(sidecarPath))) return

  const tempDir = path.join(TEMP_DIR, name)

  const tempFile = path.join(tempDir, tmpFile)

  const tempExe = path.join(tempDir, exeFile)

  await fs.mkdirp(tempDir)

  try {
    if (!(await fs.pathExists(tempFile))) {
      await downloadFile(downloadURL, tempFile)
    }
    if (tmpFile.endsWith('.zip')) {
      const zip = new AdmZip(tempFile)

      let entryName
      zip.getEntries().forEach((entry) => {
        consola.debug(colorize`"{green ${name}}" entry name ${entry.entryName}`)
        if (
          (entry.entryName.includes(name) &&
            entry.entryName.endsWith('.exe')) ||
          (entry.entryName.includes(
            name
              .split('-')
              .filter((o) => o !== 'alpha')
              .join('-'),
          ) &&
            entry.entryName.endsWith('.exe'))
        ) {
          entryName = entry.entryName
        }
      })

      zip.extractAllTo(tempDir, true)

      if (!entryName) {
        throw new Error('cannot find exe file in zip')
      }

      await fs.rename(path.join(tempDir, entryName), tempExe)

      await fs.rename(tempExe, sidecarPath)

      consola.debug(colorize`{green "${name}"} unzip finished`)
    } else if (tmpFile.endsWith('.tar.gz')) {
      // decompress and untar the file
      await tar.x({
        file: tempFile,
        cwd: tempDir,
      })
      await fs.rename(tempExe, sidecarPath)
      consola.debug(colorize`{green "${name}"} untar finished`)
    } else if (tmpFile.endsWith('.gz')) {
      // gz
      const readStream = fs.createReadStream(tempFile)

      const writeStream = fs.createWriteStream(sidecarPath)

      await new Promise<void>((resolve, reject) => {
        const onError = (error: any) => {
          consola.error(colorize`"${name}" gz failed:`, error)
          reject(error)
        }
        readStream
          .pipe(zlib.createGunzip().on('error', onError))
          .pipe(writeStream)
          .on('finish', () => {
            consola.debug(colorize`{green "${name}"} gunzip finished`)

            execSync(`chmod 755 ${sidecarPath}`)

            consola.debug(colorize`{green "${name}"}chmod binary finished`)

            resolve()
          })
          .on('error', onError)
      })
    } else {
      // Common Files
      await fs.rename(tempFile, sidecarPath)

      consola.info(colorize`{green "${name}"} rename finished`)

      if (platform !== 'win32') {
        execSync(`chmod 755 ${sidecarPath}`)

        consola.info(colorize`{green "${name}"} chmod binary finished`)
      }
    }
    consola.success(colorize`resolve {green ${name}} finished`)
  } catch (err) {
    // 需要删除文件
    await fs.remove(sidecarPath)

    throw err
  } finally {
    // delete temp dir
    await fs.remove(tempDir)
  }
}
