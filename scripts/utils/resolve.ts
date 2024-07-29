import path from "path";
import AdmZip from "adm-zip";
import fs from "fs-extra";
import { BinInfo } from "types";
import { downloadFile, resolveSidecar } from "./download";
import { TAURI_APP_DIR, TEMP_DIR } from "./env";
import { colorize, consola } from "./logger";
import {
  getClashBackupInfo,
  getClashMetaAlphaInfo,
  getClashMetaInfo,
  getClashRustInfo,
  getNyanpasuServiceInfo,
} from "./resource";

/**
 * download the file to the resources dir
 */
export const resolveResource = async (
  binInfo: { file: string; downloadURL: string },
  options?: { force?: boolean },
) => {
  const { file, downloadURL } = binInfo;

  const resDir = path.join(TAURI_APP_DIR, "resources");

  const targetPath = path.join(resDir, file);

  if (!options?.force && (await fs.pathExists(targetPath))) return;

  await fs.mkdirp(resDir);

  await downloadFile(downloadURL, targetPath);

  consola.success(colorize`resolve {green ${file}} finished`);
};

export class Resolve {
  private infoOption: {
    platform: string;
    arch: string;
    sidecarHost: string;
  };

  constructor(
    private readonly options: {
      force?: boolean;
      platform: string;
      arch: string;
      sidecarHost: string;
    },
  ) {
    this.infoOption = {
      platform: this.options.platform,
      arch: this.options.arch,
      sidecarHost: this.options.sidecarHost,
    };
  }

  /**
   * only Windows
   * get the wintun.dll (not required)
   */
  public async wintun() {
    const { platform } = process;

    if (platform !== "win32") return;

    const url = "https://www.wintun.net/builds/wintun-0.14.1.zip";

    const tempDir = path.join(TEMP_DIR, "wintun");

    const tempZip = path.join(tempDir, "wintun.zip");

    const wintunPath = path.join(tempDir, "wintun/bin/amd64/wintun.dll");

    const targetPath = path.join(TAURI_APP_DIR, "resources", "wintun.dll");

    if (!this.options?.force && (await fs.pathExists(targetPath))) return;

    await fs.mkdirp(tempDir);

    if (!(await fs.pathExists(tempZip))) {
      await downloadFile(url, tempZip);
    }

    // unzip
    const zip = new AdmZip(tempZip);

    zip.extractAllTo(tempDir, true);

    if (!(await fs.pathExists(wintunPath))) {
      throw new Error(`path not found "${wintunPath}"`);
    }

    await fs.rename(wintunPath, targetPath);

    await fs.remove(tempDir);

    consola.success(colorize`resolve {green wintun.dll} finished`);
  }

  public async service() {
    return await this.sidecar(getNyanpasuServiceInfo(this.infoOption));
  }

  public mmdb() {
    return resolveResource({
      file: "Country.mmdb",
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb`,
    });
  }

  public geosite() {
    return resolveResource({
      file: "geosite.dat",
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat`,
    });
  }

  public geoip() {
    return resolveResource({
      file: "geoip.dat",
      downloadURL: `https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat`,
    });
  }

  public enableLoopback() {
    return resolveResource({
      file: "enableLoopback.exe",
      downloadURL: `https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe`,
    });
  }

  private sidecar(binInfo: BinInfo | PromiseLike<BinInfo>) {
    return resolveSidecar(binInfo, this.options.platform, {
      force: this.options.force,
    });
  }

  public async clash() {
    return await this.sidecar(getClashBackupInfo(this.infoOption));
  }

  public async clashMeta() {
    return await this.sidecar(getClashMetaInfo(this.infoOption));
  }

  public async clashMetaAlpha() {
    return await this.sidecar(getClashMetaAlphaInfo(this.infoOption));
  }

  public async clashRust() {
    return await this.sidecar(getClashRustInfo(this.infoOption));
  }
}
