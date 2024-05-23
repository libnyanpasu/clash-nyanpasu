import { CLASH_MANIFEST } from "../manifest/clash-premium";
import { colorize, consola } from "./logger";
import { CLASH_META_MANIFEST } from "../manifest/clash-meta";

export const archCheck = (platform: string, arch: string) => {
  consola.debug(colorize`platform {yellow ${platform}}`);

  consola.debug(colorize`arch {yellow ${arch}}`);

  if (!CLASH_MANIFEST.BIN_MAP[`${platform}-${arch}`]) {
    throw new Error(`clash unsupported platform "${platform}-${arch}"`);
  }

  if (!CLASH_META_MANIFEST.BIN_MAP[`${platform}-${arch}`]) {
    throw new Error(`clash meta unsupported platform "${platform}-${arch}"`);
  }
};
