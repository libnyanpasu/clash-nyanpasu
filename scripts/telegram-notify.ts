// import { Telegraf } from "telegraf";
import { existsSync } from "fs";
import path from "path";
import { mkdirp } from "fs-extra";
import pRetry from "p-retry";
import { getOctokit } from "@actions/github";
import { version } from "../package.json";
import { array2text } from "./utils";
import { downloadFile } from "./utils/download";
import { TEMP_DIR } from "./utils/env";
import { consola } from "./utils/logger";
import { GIT_SHORT_HASH } from "./utils/shell";
import { client } from "./utils/telegram";

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
  ".exe",
  "portable.zip",
  ".rpm",
  ".deb",
  ".dmg",
  ".AppImage",
];

const isValidFormat = (fileName: string): boolean => {
  return resourceFormats.some((format) => fileName.endsWith(format));
};

const repoInfo = {
  owner: "LibNyanpasu",
  repo: "clash-nyanpasu",
};

(async () => {
  await client.start({
    botAuthToken: TELEGRAM_TOKEN,
  });

  const github = getOctokit(GITHUB_TOKEN);

  const content = nightlyBuild
    ? await github.rest.repos.getReleaseByTag({
        ...repoInfo,
        tag: "pre-release",
      })
    : await github.rest.repos.getLatestRelease(repoInfo);

  const downloadTasks: Promise<void>[] = [];

  const resourceMapping: string[] = [];

  content.data.assets.forEach((asset) => {
    if (isValidFormat(asset.name)) {
      const _path = path.join(TEMP_DIR, asset.name);

      resourceMapping.push(_path);

      downloadTasks.push(
        pRetry(() => downloadFile(asset.browser_download_url, _path), {
          retries: 5,
        }),
      );
    }
  });

  try {
    mkdirp(TEMP_DIR);

    await Promise.all(downloadTasks);
  } catch (error) {
    consola.error(error);
    throw new Error("Error during download or upload tasks");
  }

  resourceMapping.forEach((item) => {
    consola.log(`exited ${item}:`, existsSync(item));
  });

  consola.start("Staring upload tasks (nightly)");

  await pRetry(
    () =>
      client.sendFile(TELEGRAM_TO_NIGHTLY, {
        file: resourceMapping,
        forceDocument: true,
        caption: `Clash Nyanpasu Nightly Build ${GIT_SHORT_HASH}`,
        workers: 16,
        progressCallback: (progress) => consola.debug(`Uploading ${progress}`),
      }),
    { retries: 5 },
  );

  consola.success("Upload finished (nightly)");

  if (!nightlyBuild) {
    consola.start("Staring upload tasks (release)");

    await client.sendMessage(TELEGRAM_TO, {
      message: array2text([
        `Clash Nyanpasu ${version} Released!`,
        "",
        "Check out on GitHub:",
        ` - https://github.com/LibNyanpasu/clash-nyanpasu/releases/tag/v${version}`,
      ]),
    });

    consola.success("Upload finished (release)");
  }

  await client.disconnect();

  process.exit();
})().catch((error) => {
  consola.fatal(error);
  process.exit(1);
});
