import { HttpsProxyAgent } from "https-proxy-agent";
import {
  CLASH_META_ALPHA_MANIFEST,
  CLASH_META_MANIFEST,
} from "../manifest/clash-meta";
import { CLASH_MANIFEST } from "../manifest/clash-premium";
import { CLASH_RS_MANIFEST } from "../manifest/clash-rs";
import fetch, { type RequestInit } from "node-fetch";
import { BinInfo } from "types";

export const getClashInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: string;
  arch: string;
  sidecarHost?: string;
}): BinInfo => {
  const { BIN_MAP, URL_PREFIX, LATEST_DATE } = CLASH_MANIFEST;

  const name = BIN_MAP[`${platform}-${arch}`];

  const isWin = platform === "win32";

  const urlExt = isWin ? "zip" : "gz";

  const downloadURL = `${URL_PREFIX}${name}-${LATEST_DATE}.${urlExt}`;

  const exeFile = `${name}${isWin ? ".exe" : ""}`;

  const tmpFile = `${name}.${urlExt}`;

  const targetFile = `clash-${sidecarHost}${isWin ? ".exe" : ""}`;

  return {
    name: "clash",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};

export const getClashBackupInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: string;
  arch: string;
  sidecarHost?: string;
}): BinInfo => {
  const { BIN_MAP, BACKUP_URL_PREFIX, BACKUP_LATEST_DATE } = CLASH_MANIFEST;

  const name = BIN_MAP[`${platform}-${arch}`];

  const isWin = platform === "win32";

  const urlExt = isWin ? "zip" : "gz";

  const downloadURL = `${BACKUP_URL_PREFIX}${BACKUP_LATEST_DATE}/${name}-n${BACKUP_LATEST_DATE}.${urlExt}`;

  const exeFile = `${name}${isWin ? ".exe" : ""}`;

  const tmpFile = `${name}.${urlExt}`;

  const targetFile = `clash-${sidecarHost}${isWin ? ".exe" : ""}`;

  return {
    name: "clash",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};

export const getClashMetaInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: string;
  arch: string;
  sidecarHost?: string;
}): BinInfo => {
  const { BIN_MAP, URL_PREFIX, VERSION } = CLASH_META_MANIFEST;

  const name = BIN_MAP[`${platform}-${arch}`];

  const isWin = platform === "win32";

  const urlExt = isWin ? "zip" : "gz";

  const downloadURL = `${URL_PREFIX}/${name}-${VERSION}.${urlExt}`;

  const exeFile = `${name}${isWin ? ".exe" : ""}`;

  const tmpFile = `${name}-${VERSION}.${urlExt}`;

  const targetFile = `mihomo-${sidecarHost}${isWin ? ".exe" : ""}`;

  return {
    name: "mihomo",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};

export const getClashMetaAlphaInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: string;
  arch: string;
  sidecarHost?: string;
}): BinInfo => {
  const { BIN_MAP, URL_PREFIX, VERSION } = CLASH_META_ALPHA_MANIFEST;

  const name = BIN_MAP[`${platform}-${arch}`];

  const isWin = platform === "win32";

  const urlExt = isWin ? "zip" : "gz";

  const downloadURL = `${URL_PREFIX}/${name}-${VERSION}.${urlExt}`;

  const exeFile = `${name}${isWin ? ".exe" : ""}`;

  const tmpFile = `${name}-${VERSION}.${urlExt}`;

  const targetFile = `mihomo-alpha-${sidecarHost}${isWin ? ".exe" : ""}`;

  return {
    name: "mihomo-alpha",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};

export const getClashRustInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: string;
  arch: string;
  sidecarHost?: string;
}): BinInfo => {
  const { BIN_MAP, URL_PREFIX, VERSION } = CLASH_RS_MANIFEST;

  const name = BIN_MAP[`${platform}-${arch}`];

  const isWin = platform === "win32";

  const exeFile = `${name}${isWin ? ".exe" : ""}`;

  const downloadURL = `${URL_PREFIX}${VERSION}/${name}${isWin ? ".exe" : ""}`;

  const tmpFile = `${name}${isWin ? ".exe" : ""}`;

  const targetFile = `clash-rs-${sidecarHost}${isWin ? ".exe" : ""}`;

  return {
    name: "clash-rs",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};

export const getMetaAlphaLatestVersion = async () => {
  const { VERSION_URL } = CLASH_META_ALPHA_MANIFEST;

  try {
    const opts = {} as Partial<RequestInit>;

    const httpProxy =
      process.env.HTTP_PROXY ||
      process.env.http_proxy ||
      process.env.HTTPS_PROXY ||
      process.env.https_proxy;

    if (httpProxy) {
      opts.agent = new HttpsProxyAgent(httpProxy);
    }

    const response = await fetch(VERSION_URL!, {
      method: "GET",
      ...opts,
    });

    const v = await response.text();

    console.log(`Latest release version: ${VERSION_URL}`);

    return v.trim();
  } catch (error) {
    console.error("Error fetching latest release version:", error);

    process.exit(1);
  }
};
