import { Telegraf } from "telegraf";
import { downloadFileToBuffer } from "./utils/fetch";
import { consola } from "./utils/logger";
import { execSync } from "child_process";
import { array2text } from "./utils";
import { context, getOctokit } from "@actions/github";

export const sendReleaseNotify = async () => {
  if (!process.env.TELEGRAM_TOKEN) {
    throw new Error("TELEGRAM_TOKEN is required");
  }

  if (!process.env.TELEGRAM_TO) {
    throw new Error("TELEGRAM_TO is required");
  }

  if (!process.env.GITHUB_TOKEN) {
    throw new Error("GITHUB_TOKEN is required");
  }

  const github = getOctokit(process.env.GITHUB_TOKEN);

  const release = await github.rest.repos.getLatestRelease(context.repo);

  const fileList: {
    name: string;
    url: string;
    buffer: Buffer;
  }[] = [];

  for (const item of release.data.assets) {
    const supportedExtensions = [
      "x64-setup.exe",
      "x64_portable.zip",
      "amd64.AppImage",
      "amd64.deb",
      "x64.dmg",
      "aarch64.dmg",
    ];

    if (supportedExtensions.some((ext) => item.name.endsWith(ext))) {
      consola.log("Download file: " + item.name);

      const buffer = await downloadFileToBuffer(item.browser_download_url);

      fileList.push({
        name: item.name,
        url: item.browser_download_url,
        buffer: buffer,
      });
    }
  }

  const bot = new Telegraf(process.env.TELEGRAM_TOKEN);

  consola.log("Send media to Nyanpasu channel");

  const GIT_SHORT_HASH = execSync("git rev-parse --short HEAD")
    .toString()
    .trim();

  const caption = array2text([
    "Clash Nyanpasu Nightly Build",
    "",
    `Current git short hash: ${GIT_SHORT_HASH}`,
  ]);

  bot.telegram.sendMediaGroup(
    process.env.TELEGRAM_TO,
    fileList.map((file, index) => {
      return {
        type: "document",
        caption: index == fileList.length ? caption : undefined,
        media: {
          source: file.buffer,
          filename: file.name,
        },
      };
    }),
  );
};

sendReleaseNotify();
