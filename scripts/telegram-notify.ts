import { Telegraf } from "telegraf";
import pkgJson from "../package.json";
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

  const bot = new Telegraf(process.env.TELEGRAM_TOKEN);

  const { version } = pkgJson;

  const stringEscape = (str: string) => {
    return str.replace(/\./g, "\\.").replace(/-/g, "\\-").replace(/_/g, "\\_");
  };

  const github = getOctokit(process.env.GITHUB_TOKEN);

  const { data: tags } = await github.rest.repos.listTags(context.repo);

  bot.telegram.sendMessage(
    process.env.TELEGRAM_TO,
    array2text([
      `Clash Nyanpasu ${stringEscape(version)} Released\!`,
      "",
      `[Check out updates on GitHub](https://github.com/LibNyanpasu/clash-nyanpasu/compare/v${tags[1]?.name}...v${version})`,
      "",
      "*Download Link:*",
      ` - https://github.com/LibNyanpasu/clash-nyanpasu/releases/tag/v${version}`,
    ]),
    {
      parse_mode: "MarkdownV2",
    },
  );
};

sendReleaseNotify();
