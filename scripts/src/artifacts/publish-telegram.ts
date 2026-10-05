import { parseArgs } from "jsr:@std/cli@1/parse-args";
import * as path from "jsr:@std/path";
import {
  publishTelegramInventory,
  readTelegramInventory,
  registerTelegramInventory,
  type TelegramPublicationRecord,
} from "./telegram-publication.ts";

async function main() {
  const args = parseArgs(Deno.args, {
    string: ["publication-dir", "report", "target"],
    boolean: ["register-only", "verify-only"],
  });
  if (!args["publication-dir"] || !args.report) {
    throw new Error("Expected --publication-dir <dir> --report <json>");
  }
  const required = (name: string) => {
    const value = Deno.env.get(name)?.trim();
    if (!value) throw new Error(`${name} is required`);
    return value;
  };
  const apiId = Number(required("TELEGRAM_API_ID"));
  if (!Number.isSafeInteger(apiId) || apiId <= 0) {
    throw new Error("Invalid TELEGRAM_API_ID");
  }
  const apiHash = required("TELEGRAM_API_HASH");
  const token = required("TELEGRAM_TOKEN");
  const archiveToken = Deno.env.get("ARCHIVE_UPLOAD_TOKEN")?.trim() ||
    Deno.env.get("FILE_SERVER_TOKEN")?.trim() || required("UPLOAD_TOKEN");
  const chat = Deno.env.get("TELEGRAM_TO") || "@ClashNyanpasu";
  if (chat !== "@ClashNyanpasu") {
    throw new Error("Publication channel must be @ClashNyanpasu");
  }
  const inventory = await readTelegramInventory(args["publication-dir"]);
  const targets = args.target && args.target !== "all"
    ? inventory.filter((target) => target.target === args.target)
    : inventory;
  if (!targets.length) throw new Error("Unknown Telegram target");
  let previous: TelegramPublicationRecord[] = [];
  try {
    const report = JSON.parse(await Deno.readTextFile(args.report));
    if (report.chat !== chat || report.schemaVersion !== 1) {
      throw new Error("Incompatible Telegram report");
    }
    previous = report.artifacts;
    if (
      !Array.isArray(previous) ||
      previous.some((record) =>
        !inventory.some((target) => target.buildId === record.buildId)
      )
    ) {
      throw new Error("Saved Telegram receipts belong to another publication");
    }
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  }
  // Load the network adapter only after local validation; imports never authenticate.
  const { TelegramClient, Api } = await import("npm:telegram@2.26.22");
  const { StringSession } = await import(
    "npm:telegram@2.26.22/sessions/index.js"
  );
  const { CustomFile } = await import("npm:telegram@2.26.22/client/uploads.js");
  const client = new TelegramClient(new StringSession(""), apiId, apiHash, {
    connectionRetries: 3,
    requestRetries: 3,
    floodSleepThreshold: 60,
  });
  await Deno.mkdir(path.dirname(path.resolve(args.report)), {
    recursive: true,
  });
  try {
    await client.start({ botAuthToken: token });
    const entity = await client.getInputEntity(chat);
    for (
      const record of previous.filter((record) =>
        record.status === "uploaded" && record.receipt
      )
    ) {
      const [message] = await client.getMessages(entity, {
        ids: [record.receipt!.messageId],
      });
      const document = message?.media instanceof Api.MessageMediaDocument
        ? message.media.document
        : undefined;
      if (
        !(document instanceof Api.Document) ||
        document.id.toString() !== record.receipt!.documentId ||
        document.size.toJSNumber() !== record.fileSize
      ) {
        throw new Error(
          `Telegram receipt is unavailable or changed: ${record.fileName}`,
        );
      }
    }
    const records = await publishTelegramInventory(
      targets,
      previous,
      async (artifact, target) => {
        console.log(
          `[telegram] ${target.target}: ${artifact.fileName} (${artifact.fileSize} bytes)`,
        );
        if (args["register-only"] || args["verify-only"]) {
          throw new Error(
            "Saved Telegram receipt is required; upload mode repairs missing documents",
          );
        }
        let lastProgress = 0;
        const message = await client.sendFile(entity, {
          file: new CustomFile(
            artifact.fileName,
            artifact.fileSize,
            artifact.path,
          ),
          forceDocument: true,
          workers: 8,
          caption: `Clash Nyanpasu ${
            target.channel === "release"
              ? target.tag
              : `Nightly ${target.commit.slice(0, 8)}`
          }\n${target.target}\nSHA-256: ${artifact.sha256}`,
          formattingEntities: [],
          progressCallback: (progress: number) => {
            if (Date.now() - lastProgress >= 30_000 || progress === 1) {
              console.log(
                `[telegram] ${artifact.fileName}: ${
                  Math.round(progress * 100)
                }%`,
              );
              lastProgress = Date.now();
            }
          },
        });
        const document = message.media instanceof Api.MessageMediaDocument
          ? message.media.document
          : undefined;
        if (
          !(document instanceof Api.Document) ||
          document.size.toJSNumber() !== artifact.fileSize
        ) {
          throw new Error(
            `Telegram did not confirm document size: ${artifact.fileName}`,
          );
        }
        return {
          messageId: message.id,
          documentId: document.id.toString(),
          messageUrl: `https://t.me/ClashNyanpasu/${message.id}`,
        };
      },
      async (artifacts) => {
        await Deno.writeTextFile(
          args.report!,
          JSON.stringify(
            {
              schemaVersion: 1,
              chat,
              targets: targets.map(({ artifacts: _artifacts, ...metadata }) =>
                metadata
              ),
              artifacts,
            },
            null,
            2,
          ),
        );
      },
    );
    if (!args["verify-only"]) {
      await registerTelegramInventory(targets, records, archiveToken);
    }
    const failed = records.filter((record) => record.status === "failed");
    if (failed.length) {
      throw new Error(
        `${failed.length} Telegram upload(s) failed; inspect ${args.report}`,
      );
    }
    console.log(`[telegram] ${records.length} documents published`);
  } finally {
    await client.destroy();
  }
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
