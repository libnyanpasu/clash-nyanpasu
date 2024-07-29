import path from "path";

export const cwd = process.cwd();
export const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
export const MANIFEST_DIR = path.join(cwd, "manifest");
export const GITHUB_PROXY = "https://mirror.ghproxy.com/";
export const GITHUB_TOKEN = process.env.GITHUB_TOKEN;
export const TEMP_DIR = path.join(cwd, "node_modules/.verge");
export const MANIFEST_VERSION_PATH = path.join(MANIFEST_DIR, "version.json");
