import { retry } from "jsr:@std/async@1/retry";
import { format as formatBytes } from "jsr:@std/fmt@1/bytes";
import { ensureDir, exists } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
import { Bot } from "npm:grammy";
import {
  UPLOAD_CONCURRENCY,
  uploadAllFiles,
  UploadResult,
} from "./utils/file-server.ts";
import { consola } from "./utils/logger.ts";

// --- env helpers ---

function requireEnv(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    consola.fatal(`${name} is required`);
    Deno.exit(1);
  }
  return value;
}

const nightlyBuild = Deno.args.includes("--nightly");
const fromLocal = Deno.args.includes("--from-local");

const TELEGRAM_TOKEN = requireEnv("TELEGRAM_TOKEN");
const TELEGRAM_TO = requireEnv("TELEGRAM_TO");
const TELEGRAM_TO_NIGHTLY = requireEnv("TELEGRAM_TO_NIGHTLY");
const GITHUB_TOKEN = requireEnv("GITHUB_TOKEN");
const FILE_SERVER_TOKEN = fromLocal ? "" : requireEnv("FILE_SERVER_TOKEN");
const WORKFLOW_RUN_ID = Deno.env.get("WORKFLOW_RUN_ID");
const UPLOAD_RESULTS_DIR = Deno.env.get("UPLOAD_RESULTS_DIR") || ".";

// --- constants ---

const WORKSPACE_ROOT = path.join(import.meta.dirname!, "..");
const TEMP_DIR = path.join(WORKSPACE_ROOT, "node_modules/.verge");

const repoInfo = { owner: "libnyanpasu", repo: "clash-nyanpasu" } as const;

const resourceFormats = [
  ".exe",
  "portable.zip",
  ".rpm",
  ".deb",
  ".dmg",
  ".AppImage",
];

// --- helpers ---

function isValidFormat(fileName: string): boolean {
  return resourceFormats.some((fmt) => fileName.endsWith(fmt));
}

function getFileSize(filePath: string): string {
  const stat = Deno.statSync(filePath);
  return formatBytes(stat.size ?? 0);
}

function getGitShortHash(): string {
  const cmd = new Deno.Command("git", {
    args: ["rev-parse", "--short", "HEAD"],
    stdout: "piped",
  });
  const { stdout } = cmd.outputSync();
  return new TextDecoder().decode(stdout).trim();
}

async function getVersion(): Promise<string> {
  const pkgPath = path.join(WORKSPACE_ROOT, "package.json");
  const pkg = JSON.parse(await Deno.readTextFile(pkgPath));
  return pkg.version as string;
}

async function downloadFile(url: string, destPath: string): Promise<void> {
  consola.debug(`download "${url}" to "${destPath}"`);

  const response = await fetch(url, {
    method: "GET",
    headers: {
      "Content-Type": "application/octet-stream",
      "User-Agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0",
    },
  });

  if (!response.ok) {
    throw new Error(`download failed: ${response.statusText}`);
  }

  const buffer = new Uint8Array(await response.arrayBuffer());
  await Deno.writeFile(destPath, buffer);

  consola.success(`download finished "${url.split("/").at(-1)}"`);
}

interface GitHubAsset {
  name: string;
  browser_download_url: string;
}

interface GitHubRelease {
  assets: GitHubAsset[];
}

async function fetchRelease(): Promise<GitHubRelease> {
  const { owner, repo } = repoInfo;
  const url = nightlyBuild
    ? `https://api.github.com/repos/${owner}/${repo}/releases/tags/pre-release`
    : `https://api.github.com/repos/${owner}/${repo}/releases/latest`;

  const resp = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${GITHUB_TOKEN}`,
      "X-GitHub-Api-Version": "2022-11-28",
    },
  });

  if (!resp.ok) {
    throw new Error(`GitHub API error: ${resp.status} ${resp.statusText}`);
  }

  return (await resp.json()) as GitHubRelease;
}

async function readLocalUploadResults(dir: string): Promise<UploadResult[]> {
  const results: UploadResult[] = [];
  try {
    for await (const entry of Deno.readDir(dir)) {
      if (entry.isDirectory) {
        const jsonPath = path.join(dir, entry.name, "upload-results.json");
        try {
          const content = await Deno.readTextFile(jsonPath);
          const parsed = JSON.parse(content) as UploadResult[];
          results.push(...parsed);
          consola.success(`Loaded ${parsed.length} results from ${entry.name}`);
        } catch {
          // No upload-results.json in this subdirectory, skip
        }
      }
    }
  } catch (err) {
    consola.warn(`Could not read directory ${dir}: ${err}`);
  }
  return results;
}

// --- platform grouping ---

interface PlatformGroup {
  label: string;
  filter: (filePath: string) => boolean;
}

const platformGroups: PlatformGroup[] = [
  {
    label: "Windows",
    filter: (item) =>
      !item.includes("fixed-webview") &&
      (item.endsWith(".exe") || item.endsWith("portable.zip")),
  },
  {
    label: "macOS",
    filter: (item) => item.endsWith(".dmg"),
  },
  {
    label: "Linux",
    filter: (item) =>
      (item.endsWith(".rpm") ||
        item.endsWith(".deb") ||
        item.endsWith(".AppImage")) &&
      !item.includes("armel") &&
      !item.includes("armhf"),
  },
  {
    label: "Linux (armv7)",
    filter: (item) =>
      (item.endsWith(".rpm") ||
        item.endsWith(".deb") ||
        item.endsWith(".AppImage")) &&
      (item.includes("armel") || item.includes("armhf")),
  },
];

// --- main ---

async function main() {
  const bot = new Bot(TELEGRAM_TOKEN);

  const GIT_SHORT_HASH = getGitShortHash();
  const version = await getVersion();

  let uploadResults: UploadResult[];

  if (fromLocal) {
    consola.info(
      `Reading upload results from local directory: ${UPLOAD_RESULTS_DIR}`,
    );
    uploadResults = await readLocalUploadResults(UPLOAD_RESULTS_DIR);
    consola.success(`Loaded ${uploadResults.length} total upload results`);
  } else {
    const release = await fetchRelease();
    const resourceMapping: string[] = [];
    const downloadTasks: Promise<void>[] = [];

    for (const asset of release.assets) {
      if (isValidFormat(asset.name)) {
        const dest = path.join(TEMP_DIR, asset.name);
        resourceMapping.push(dest);
        downloadTasks.push(
          retry(() => downloadFile(asset.browser_download_url, dest), {
            maxAttempts: 5,
          }),
        );
      }
    }

    try {
      await ensureDir(TEMP_DIR);
      await Promise.all(downloadTasks);
    } catch (error) {
      consola.error(error);
      throw new Error("Error during download tasks");
    }

    for (const item of resourceMapping) {
      consola.log(
        `found ${item}, size: ${getFileSize(item)}`,
        await exists(item),
      );
    }

    const buildType = nightlyBuild ? "nightly" : "release";
    const folderPath = `${buildType}/${GIT_SHORT_HASH}`;

    consola.start(
      `Uploading ${resourceMapping.length} files to file server (concurrency: ${UPLOAD_CONCURRENCY}, folder: ${folderPath})...`,
    );

    uploadResults = await uploadAllFiles(
      resourceMapping,
      FILE_SERVER_TOKEN,
      folderPath,
    );

    consola.success(`Uploaded ${uploadResults.length} files to file server`);
  }

  // build message with grouped download links
  const lines: string[] = [];

  if (!nightlyBuild) {
    lines.push(
      `**Clash Nyanpasu ${version} Released!**`,
      "",
      "GitHub Release:",
      `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v${version}`,
    );
  } else {
    lines.push(
      `**Clash Nyanpasu Nightly Build \`${GIT_SHORT_HASH}\`**`,
      "",
      "⚠️ Could be unstable, use at your own risk.",
    );
    if (WORKFLOW_RUN_ID) {
      lines.push(
        "",
        "GitHub Actions:",
        `https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/${WORKFLOW_RUN_ID}`,
      );
    }
  }

  lines.push("", "--- Download Links ---");

  for (const group of platformGroups) {
    const groupFiles = uploadResults.filter((r) => group.filter(r.fileName));
    if (groupFiles.length === 0) continue;

    lines.push("", `**${group.label}:**`);
    for (const file of groupFiles) {
      lines.push(`- [${file.fileName}](${file.downloadUrl})`);
    }
  }

  const messageText = lines.join("\n");
  const chatId = nightlyBuild ? TELEGRAM_TO_NIGHTLY : TELEGRAM_TO;

  await bot.api.sendMessage(chatId, messageText, { parse_mode: "Markdown" });
  consola.success("Sent telegram notification");
}

main().catch((error) => {
  consola.fatal(error);
  Deno.exit(1);
});
