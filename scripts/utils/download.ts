import { BinInfo } from "types";
import { colorize, consola } from "./logger";
import { HttpsProxyAgent } from "https-proxy-agent";
import fetch, { type RequestInit } from "node-fetch";
import { TAURI_APP_DIR, TEMP_DIR } from "./env";
import fs from "fs-extra";
import path from "path";
import AdmZip from "adm-zip";
import zlib from "zlib";
import { execSync } from "child_process";

/**
 * download sidecar and rename
 */
export const downloadFile = async (url: string, path: string) => {
  const options: Partial<RequestInit> = {};

  const httpProxy =
    process.env.HTTP_PROXY ||
    process.env.http_proxy ||
    process.env.HTTPS_PROXY ||
    process.env.https_proxy;

  if (httpProxy) {
    options.agent = new HttpsProxyAgent(httpProxy);
  }

  const response = await fetch(url, {
    ...options,
    method: "GET",
    headers: { "Content-Type": "application/octet-stream" },
  });

  const buffer = await response.arrayBuffer();

  await fs.writeFile(path, new Uint8Array(buffer));

  consola.success(
    colorize`download finished {gray "${url.split("/").at(-1)}"}`,
  );
};

export const resolveSidecar = async (
  binInfo: BinInfo,
  platform: string,
  option?: { force?: boolean },
) => {
  const { name, targetFile, tmpFile, exeFile, downloadURL } = binInfo;

  consola.debug(colorize`resolve {cyan ${name}}...`);

  const sidecarDir = path.join(TAURI_APP_DIR, "sidecar");

  const sidecarPath = path.join(sidecarDir, targetFile);

  await fs.mkdirp(sidecarDir);

  if (!option?.force && (await fs.pathExists(sidecarPath))) return;

  const tempDir = path.join(TEMP_DIR, name);

  const tempFile = path.join(tempDir, tmpFile);

  const tempExe = path.join(tempDir, exeFile);

  await fs.mkdirp(tempDir);

  try {
    if (!(await fs.pathExists(tempFile))) {
      await downloadFile(downloadURL, tempFile);
    }
    if (tmpFile.endsWith(".zip")) {
      const zip = new AdmZip(tempFile);

      zip.getEntries().forEach((entry) => {
        consola.debug(
          colorize`"{green ${name}}" entry name ${entry.entryName}`,
        );
      });

      zip.extractAllTo(tempDir, true);

      await fs.rename(tempExe, sidecarPath);

      consola.debug(colorize`{green "${name}"} unzip finished`);
    } else if (tmpFile.endsWith(".gz")) {
      // gz
      const readStream = fs.createReadStream(tempFile);

      const writeStream = fs.createWriteStream(sidecarPath);

      await new Promise<void>((resolve, reject) => {
        const onError = (error: any) => {
          consola.error(colorize`"${name}" gz failed:`, error);
          reject(error);
        };
        readStream
          .pipe(zlib.createGunzip().on("error", onError))
          .pipe(writeStream)
          .on("finish", () => {
            consola.debug(colorize`{green "${name}"} gunzip finished`);

            execSync(`chmod 755 ${sidecarPath}`);

            consola.debug(colorize`{green "${name}"}chmod binary finished`);

            resolve();
          })
          .on("error", onError);
      });
    } else {
      // Common Files
      await fs.rename(tempFile, sidecarPath);

      consola.info(colorize`{green "${name}"} rename finished`);

      if (platform !== "win32") {
        execSync(`chmod 755 ${sidecarPath}`);

        consola.info(colorize`{green "${name}"} chmod binary finished`);
      }
    }
    consola.success(colorize`resolve {green ${name}} finished`);
  } catch (err) {
    // 需要删除文件
    await fs.remove(sidecarPath);

    throw err;
  } finally {
    // delete temp dir
    await fs.remove(tempDir);
  }
};
