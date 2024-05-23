import { ClashManifest } from "types";
import versionManifest from "../../manifest/version.json";

export const CLASH_META_MANIFEST: ClashManifest = {
  URL_PREFIX: `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
  VERSION: versionManifest.latest.mihomo,
  BIN_MAP: {
    "win32-x64": "mihomo-windows-amd64-compatible",
    "darwin-x64": "mihomo-darwin-amd64-compatible",
    "darwin-arm64": "mihomo-darwin-arm64",
    "linux-x64": "mihomo-linux-amd64-compatible",
    "linux-arm64": "mihomo-linux-arm64",
  },
};

export const CLASH_META_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
  URL_PREFIX:
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha",
  VERSION: versionManifest.latest.mihomo_alpha,
  BIN_MAP: {
    "win32-x64": "mihomo-windows-amd64-compatible",
    "darwin-x64": "mihomo-darwin-amd64-compatible",
    "darwin-arm64": "mihomo-darwin-arm64",
    "linux-x64": "mihomo-linux-amd64-compatible",
    "linux-arm64": "mihomo-linux-arm64",
  },
};
