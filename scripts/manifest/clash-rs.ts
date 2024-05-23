import { ClashManifest } from "types";
import versionManifest from "../../manifest/version.json";

export const CLASH_RS_MANIFEST: ClashManifest = {
  URL_PREFIX: "https://github.com/Watfaq/clash-rs/releases/download/",
  VERSION: versionManifest.latest.clash_rs,
  BIN_MAP: {
    "win32-x64": "clash-x86_64-pc-windows-msvc",
    "darwin-x64": "clash-x86_64-apple-darwin",
    "darwin-arm64": "clash-aarch64-apple-darwin",
    "linux-x64": "clash-x86_64-unknown-linux-gnu-static-crt",
    "linux-arm64": "clash-aarch64-unknown-linux-gnu-static-crt",
  },
};
