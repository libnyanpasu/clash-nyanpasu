import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  publishTelegramInventory,
  registerTelegramInventory,
  type TelegramPublicationRecord,
  type TelegramTarget,
} from "./telegram-publication.ts";

const target: TelegramTarget = {
  buildId: `123-1-${"a".repeat(40)}-windows-x86_64`,
  folderPath: "release/v2.0.0",
  channel: "release",
  commit: "a".repeat(40),
  tag: "v2.0.0",
  target: "windows-x86_64",
  publishedAt: "2026-10-06T00:00:00.000Z",
  artifacts: ["installer.exe", "portable.zip"].map((fileName) => ({
    fileName,
    fileSize: 3,
    sha256: "b".repeat(64),
    md5: "c".repeat(32),
    path: `/tmp/${fileName}`,
  })),
};
const receipt = {
  messageId: 123,
  documentId: "456",
  messageUrl: "https://t.me/ClashNyanpasu/123",
};

Deno.test("historical MTProto adapter loads in Deno without authentication or lifecycle scripts", async () => {
  const { TelegramClient } = await import("npm:telegram@2.26.22");
  const { StringSession } = await import(
    "npm:telegram@2.26.22/sessions/index.js"
  );
  const { CustomFile } = await import("npm:telegram@2.26.22/client/uploads.js");
  const file = new CustomFile("installer.exe", 3, "/tmp/installer.exe");
  assertEquals(file.size, 3);
  const client = new TelegramClient(
    new StringSession(""),
    123,
    "a".repeat(32),
    {},
  );
  await client.destroy();
});

Deno.test("Telegram checkpoints success before a later failure and resumes only missing files", async () => {
  const checkpoints: TelegramPublicationRecord[][] = [];
  const records = await publishTelegramInventory(
    [target],
    [],
    async (artifact) => {
      if (artifact.fileName === "portable.zip") {
        throw new Error(
          "Connection lost",
        );
      }
      return receipt;
    },
    async (records) => {
      checkpoints.push([...records]);
    },
  );
  assertEquals(checkpoints[0][0].receipt, receipt);
  assertEquals(records.map((record) => record.status), ["uploaded", "failed"]);
  const calls: string[] = [];
  const resumed = await publishTelegramInventory(
    [target],
    records,
    async (artifact) => {
      calls.push(artifact.fileName);
      return { ...receipt, messageId: 124 };
    },
    async () => {},
  );
  assertEquals(calls, ["portable.zip"]);
  assertEquals(resumed.every((record) => record.status === "uploaded"), true);
});

Deno.test("Telegram rejects byte conflicts before posting and retains unprocessed receipts", async () => {
  const prior: TelegramPublicationRecord[] = target.artifacts.map((
    artifact,
    index,
  ) => ({
    ...artifact,
    buildId: target.buildId,
    target: target.target,
    status: "uploaded",
    receipt: { ...receipt, messageId: 123 + index },
  }));
  let sent = false;
  await assertRejects(
    () =>
      publishTelegramInventory([target], [prior[0], {
        ...prior[1],
        sha256: "d".repeat(64),
      }], async () => {
        sent = true;
        return receipt;
      }, async () => {}),
    Error,
    "Conflicting Telegram receipt",
  );
  assertEquals(sent, false);
  await publishTelegramInventory(
    [target],
    prior,
    async () => receipt,
    async (records) => {
      assertEquals(records.length, 2);
    },
  );
});

Deno.test("Telegram archive registration excludes incomplete targets and local paths", async () => {
  const records = await publishTelegramInventory(
    [target],
    [],
    async (_artifact) => receipt,
    async () => {},
  );
  const calls: Record<string, unknown>[] = [];
  const fetcher: typeof fetch = async (_url, init) => {
    assertEquals(
      new Headers(init?.headers).get("authorization"),
      "Bearer test",
    );
    const body = JSON.parse(String(init?.body));
    calls.push(body);
    assertEquals(body.storageProvider, "telegram");
    assertEquals(body.schemaVersion, 2);
    assertEquals(body.itemIdentifier, "ClashNyanpasu");
    assertEquals(
      body.artifacts.some((artifact: Record<string, unknown>) =>
        "path" in artifact
      ),
      false,
    );
    return Response.json({
      buildId: body.buildId,
      status: "ready",
      artifacts: body.artifacts,
    });
  };
  await registerTelegramInventory(
    [target],
    records.slice(0, 1),
    "test",
    fetcher,
  );
  assertEquals(calls.length, 0);
  await registerTelegramInventory([target], records, "test", fetcher);
  assertEquals(calls.length, 1);
  await assertRejects(
    () =>
      registerTelegramInventory(
        [target],
        records,
        "test",
        async () =>
          Response.json({ error: "Migration missing" }, { status: 500 }),
      ),
    Error,
    "HTTP 500",
  );
  await assertRejects(
    () =>
      registerTelegramInventory(
        [target],
        records,
        "test",
        async () => new Response("<html>fallback</html>"),
      ),
    Error,
  );
});
