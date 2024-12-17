import crypto from 'node:crypto'
import path from 'path'
import AdmZip from 'adm-zip'
import fs from 'fs-extra'
import { BinInfo } from '../types'
import { downloadFile, resolveSidecar } from './download'
import { TAURI_APP_DIR, TEMP_DIR } from './env'
import { colorize, consola } from './logger'
import { NodeArch } from './manifest'
import {
  getClashBackupInfo,
  getClashMetaAlphaInfo,
  getClashMetaInfo,
  getClashRustAlphaInfo,
  getClashRustInfo,
  getNyanpasuServiceInfo,
} from './resource'

/**
 * download the file to the resources dir
 */
export const resolveResource = async (
  binInfo: { file: string; downloadURL: string },
  options?: { force?: boolean },
) => {
  const { file, downloadURL } = binInfo

  const resDir = path.join(TAURI_APP_DIR, 'resources')

  const targetPath = path.join(resDir, file)

  if (!options?.force && (await fs.pathExists(targetPath))) return

  await fs.mkdirp(resDir)

  await downloadFile(downloadURL, targetPath)

  consola.success(colorize`resolve {green ${file}} finished`)
}

export class Resolve {
  private infoOption: {
    platform: NodeJS.Platform
    arch: NodeArch
    sidecarHost: string
  }

  constructor(
    private readonly options: {
      force?: boolean
      platform: NodeJS.Platform
      arch: NodeArch
      sidecarHost: string
    },
  ) {
    this.infoOption = {
      platform: this.options.platform,
      arch: this.options.arch,
      sidecarHost: this.options.sidecarHost,
    }
  }

  /**
   * only Windows
   * get the wintun.dll (not required)
   */
  public async wintun() {
    const { platform } = process
    let arch: string = this.options.arch || 'x64'
    if (platform !== 'win32') return

    switch (arch) {
      case 'x64':
        arch = 'amd64'
        break
      case 'ia32':
        arch = 'x86'
        break
      case 'arm':
        arch = 'arm'
        break
      case 'arm64':
        arch = 'arm64'
        break
      default:
        throw new Error(`unsupported arch ${arch}`)
    }

    const url = 'https://www.wintun.net/builds/wintun-0.14.1.zip'
    const hash =
      '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51'

    const tempDir = path.join(TEMP_DIR, 'wintun')

    const tempZip = path.join(tempDir, 'wintun.zip')

    // const wintunPath = path.join(tempDir, "wintun/bin/amd64/wintun.dll");

    const targetPath = path.join(TAURI_APP_DIR, 'resources', 'wintun.dll')

    if (!this.options?.force && (await fs.pathExists(targetPath))) return

    await fs.mkdirp(tempDir)

    if (!(await fs.pathExists(tempZip))) {
      await downloadFile(url, tempZip)
    }

    // check hash
    const hashBuffer = await fs.readFile(tempZip)
    const sha256 = crypto.createHash('sha256')
    sha256.update(hashBuffer)
    const hashValue = sha256.digest('hex')
    if (hashValue !== hash) {
      throw new Error(`wintun. hash not match ${hashValue}`)
    }

    // unzip
    const zip = new AdmZip(tempZip)

    zip.extractAllTo(tempDir, true)

    // recursive list path for debug use
    const files = (await fs.readdir(tempDir, { recursive: true })).filter(
      (file) => file.includes('wintun.dll'),
    )
    consola.debug(colorize`{green wintun} founded dlls: ${files}`)

    const file = files.find((file) => file.includes(arch))
    if (!file) {
      throw new Error(`wintun. not found arch ${arch}`)
    }

    const wintunPath = path.join(tempDir, file.toString())

    if (!(await fs.pathExists(wintunPath))) {
      throw new Error(`path not found "${wintunPath}"`)
    }
    // prepare resource dir
    await fs.mkdirp(path.dirname(targetPath))
    await fs.copyFile(wintunPath, targetPath)

    await fs.remove(tempDir)

    consola.success(colorize`resolve {green wintun.dll} finished`)
  }

  public async service() {
    return await this.sidecar(getNyanpasuServiceInfo(this.infoOption))
  }

  public mmdb() {
    return resolveResource({
      file: 'Country.mmdb',
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb`,
    })
  }

  public geosite() {
    return resolveResource({
      file: 'geosite.dat',
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat`,
    })
  }

  public geoip() {
    return resolveResource({
      file: 'geoip.dat',
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat`,
    })
  }

  public enableLoopback() {
    return resolveResource({
      file: 'enableLoopback.exe',
      downloadURL: `https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe`,
    })
  }

  private sidecar(binInfo: BinInfo | PromiseLike<BinInfo>) {
    return resolveSidecar(binInfo, this.options.platform, {
      force: this.options.force,
    })
  }

  public async clash() {
    return await this.sidecar(getClashBackupInfo(this.infoOption))
  }

  public async clashMeta() {
    return await this.sidecar(getClashMetaInfo(this.infoOption))
  }

  public async clashMetaAlpha() {
    return await this.sidecar(getClashMetaAlphaInfo(this.infoOption))
  }

  public async clashRust() {
    return await this.sidecar(getClashRustInfo(this.infoOption))
  }

  public async clashRustAlpha() {
    return await this.sidecar(getClashRustAlphaInfo(this.infoOption))
  }
}
