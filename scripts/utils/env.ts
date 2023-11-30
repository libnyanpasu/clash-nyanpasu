import path from "node:path";

export const cwd = process.cwd();
export const TAURI_APP_DIR = path.join(cwd, "backend/tauri");
export const GITHUB_PROXY = "https://mirror.ghproxy.com/";
