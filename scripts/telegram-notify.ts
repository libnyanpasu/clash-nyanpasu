import { Telegraf } from "telegraf";
import pkgJson from "../package.json";
import { array2text } from "./utils";

export const sendReleaseNotify = () => {
  if (!process.env.TELEGRAM_TOKEN) {
    throw new Error("TELEGRAM_TOKEN is required");
  }

  if (!process.env.TELEGRAM_TO) {
    throw new Error("TELEGRAM_TO is required");
  }

  const bot = new Telegraf(process.env.TELEGRAM_TOKEN);

  const { version } = pkgJson;

  bot.telegram.sendMessage(
    process.env.TELEGRAM_TO,
    array2text([
      `Clash Nyanpasu ${version} Released!`,
      "",
      `[Check out updates on GitHub](https://github.com/LibNyanpasu/clash-nyanpasu/compare/v${version}...main)`,
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
