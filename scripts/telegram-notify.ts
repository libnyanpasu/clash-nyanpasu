// import { Telegraf } from "telegraf";
import { version } from "../package.json";
import { getOctokit } from "@actions/github";
import { consola } from "./utils/logger";
import { client } from "./utils/telegram";
import { downloadFile } from "./utils/download";
import path from "path";
import { TEMP_DIR } from "./utils/env";
import { GIT_SHORT_HASH } from "./utils/shell";
import { existsSync } from "fs";
import { mkdirp } from "fs-extra";

const nightlyBuild = process.argv.includes("--nightly");

if (!process.env.TELEGRAM_TOKEN) {
  throw new Error("TELEGRAM_TOKEN is required");
}

const TELEGRAM_TOKEN = process.env.TELEGRAM_TOKEN;

if (!process.env.TELEGRAM_TO) {
  throw new Error("TELEGRAM_TO is required");
}

const TELEGRAM_TO = process.env.TELEGRAM_TO;

if (!process.env.TELEGRAM_TO_NIGHTLY) {
  throw new Error("TELEGRAM_TO_NIGHTLY is required");
}

const TELEGRAM_TO_NIGHTLY = process.env.TELEGRAM_TO_NIGHTLY;

if (!process.env.GITHUB_TOKEN) {
  throw new Error("GITHUB_TOKEN is required");
}

const GITHUB_TOKEN = process.env.GITHUB_TOKEN;

const resourceFormats = [
  "x64-setup.exe",
  "x64_portable.zip",
  "amd64.AppImage",
  "amd64.deb",
  "x64.dmg",
  "aarch64.dmg",
];

const isValidFormat = (fileName: string): boolean => {
  return resourceFormats.some((format) => fileName.endsWith(format));
};

(async () => {
  await client.start({
    botAuthToken: TELEGRAM_TOKEN,
  });

  const github = getOctokit(GITHUB_TOKEN);

  const content = nightlyBuild
    ? await github.rest.repos.getReleaseByTag({
        owner: "LibNyanpasu",
        repo: "clash-nyanpasu",
        tag: "pre-release",
      })
    : await github.rest.repos.getLatestRelease();

  const downloadTasks: Promise<void>[] = [];

  const reourceMappping: string[] = [];

  content.data.assets.forEach((asset) => {
    if (isValidFormat(asset.name)) {
      const _path = path.join(TEMP_DIR, asset.name);

      reourceMappping.push(_path);

      downloadTasks.push(downloadFile(asset.browser_download_url, _path));
    }
  });

  try {
    mkdirp(TEMP_DIR);

    await Promise.all(downloadTasks);
  } catch (error) {
    consola.error(error);
    throw new Error("Error during download or upload tasks");
  }

  reourceMappping.forEach((item) => {
    consola.log(`exited ${item}:`, existsSync(item));
  });

  consola.start("Staring upload tasks (nightly)");

  await client.sendFile(TELEGRAM_TO_NIGHTLY, {
    file: reourceMappping,
    forceDocument: true,
    caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH}`,
    workers: 16,
    progressCallback: (progress) => consola.start(`Uploading ${progress}`),
  });

  consola.success("Upload finished (nightly)");

  if (!nightlyBuild) {
    consola.start("Staring upload tasks (release)");

    await client.sendFile(TELEGRAM_TO, {
      file: reourceMappping,
      forceDocument: true,
      caption: [
        `Clash Nyanpasu ${version} Released!`,
        "",
        "Check out on GitHub:",
        ` - https://github.com/LibNyanpasu/clash-nyanpasu/releases/tag/v${version}`,
      ],
      workers: 16,
      progressCallback: (progress) => consola.start(`Uploading ${progress}`),
    });

    consola.success("Upload finished (release)");
  }

  await client.disconnect();

  process.exit();
})();
