import fetch, { type RequestInit } from "node-fetch";
import { BinInfo } from "types";
import {
  CLASH_META_ALPHA_MANIFEST,
  CLASH_META_MANIFEST,
} from "../manifest/clash-meta";
import { CLASH_MANIFEST } from "../manifest/clash-premium";
import { CLASH_RS_MANIFEST } from "../manifest/clash-rs";
import { getProxyAgent } from "./";
import { SIDECAR_HOST } from "./consts";

const SERVICE_REPO = "LibNyanpasu/nyanpasu-service";

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

    const httpProxy = getProxyAgent();

    if (httpProxy) {
      opts.agent = httpProxy;
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

export const getNyanpasuServiceLatestVersion = async () => {
  try {
    const opts = {} as Partial<RequestInit>;

    const httpProxy = getProxyAgent();
    if (httpProxy) {
      opts.agent = httpProxy;
    }

    const url = new URL("https://github.com");
    url.pathname = `/${SERVICE_REPO}/releases/latest`;
    const response = await fetch(url, {
      method: "GET",
      redirect: "manual",
      ...opts,
    });

    const location = response.headers.get("location");
    if (!location) {
      throw new Error("Cannot find location from the response header");
    }
    const tag = location.split("/").pop();
    if (!tag) {
      throw new Error("Cannot find tag from the location");
    }
    console.log(`Latest release version: ${tag}`);
    return tag.trim();
  } catch (error) {
    console.error("Error fetching latest release version:", error);
    process.exit(1);
  }
};

export const getNyanpasuServiceInfo = async ({
  sidecarHost,
}: {
  sidecarHost: string;
}): Promise<BinInfo> => {
  const name = `nyanpasu-service`;
  const isWin = SIDECAR_HOST?.includes("windows");
  const urlExt = isWin ? "zip" : "tar.gz";
  // first we had to get the latest tag
  const version = await getNyanpasuServiceLatestVersion();
  const downloadURL = `https://github.com/${SERVICE_REPO}/releases/download/${version}/${name}-${sidecarHost}.${urlExt}`;
  const exeFile = `${name}${isWin ? ".exe" : ""}`;
  const tmpFile = `${name}-${sidecarHost}.${urlExt}`;
  const targetFile = `nyanpasu-service-${sidecarHost}${isWin ? ".exe" : ""}`;
  return {
    name: "clash",
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};
