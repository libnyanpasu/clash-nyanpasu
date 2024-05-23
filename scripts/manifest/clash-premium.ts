import { ClashManifest } from "types";
import versionManifest from "../../manifest/version.json";

export const CLASH_MANIFEST: ClashManifest = {
  URL_PREFIX: "https://github.com/Dreamacro/clash/releases/download/premium/",
  LATEST_DATE: "2023.08.17",
  STORAGE_PREFIX: "https://release.dreamacro.workers.dev/",
  BACKUP_URL_PREFIX:
    "https://github.com/zhongfly/Clash-premium-backup/releases/download/",
  BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
  BIN_MAP: {
    "win32-x64": "clash-windows-amd64",
    "darwin-x64": "clash-darwin-amd64",
    "darwin-arm64": "clash-darwin-arm64",
    "linux-x64": "clash-linux-amd64",
    "linux-arm64": "clash-linux-arm64",
  },
};
