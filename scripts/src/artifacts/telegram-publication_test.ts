import assert from "node:assert/strict";
import { test } from "node:test";
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
  messageUrl: "https://t.me/ExampleArchive/123",
};

test("historical MTProto adapter loads in Node without authentication or lifecycle scripts", async () => {
  const { TelegramClient } = await import("telegram");
  const { StringSession } = await import(
    "telegram/sessions/index.js"
  );
  const { CustomFile } = await import("telegram/client/uploads.js");
  const file = new CustomFile("installer.exe", 3, "/tmp/installer.exe");
  assert.deepStrictEqual(file.size, 3);
  const client = new TelegramClient(
    new StringSession(""),
    123,
    "a".repeat(32),
    {},
  );
  await client.destroy();
});

test("Telegram checkpoints success before a later failure and resumes only missing files", async () => {
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
  assert.deepStrictEqual(checkpoints[0][0].receipt, receipt);
  assert.deepStrictEqual(records.map((record) => record.status), ["uploaded", "failed"]);
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
  assert.deepStrictEqual(calls, ["portable.zip"]);
  assert.deepStrictEqual(resumed.every((record) => record.status === "uploaded"), true);
});

test("Telegram rejects byte conflicts before posting and retains unprocessed receipts", async () => {
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
  await assert.rejects(() =>
      publishTelegramInventory([target], [prior[0], {
        ...prior[1],
        sha256: "d".repeat(64),
      }], async () => {
        sent = true;
        return receipt;
      }, async () => {}), { message: /Conflicting Telegram receipt/ });
  assert.deepStrictEqual(sent, false);
  await publishTelegramInventory(
    [target],
    prior,
    async () => receipt,
    async (records) => {
      assert.deepStrictEqual(records.length, 2);
    },
  );
});

test("Telegram archive registration excludes incomplete targets and local paths", async () => {
  const records = await publishTelegramInventory(
    [target],
    [],
    async (_artifact) => receipt,
    async () => {},
  );
  const calls: Record<string, unknown>[] = [];
  const fetcher: typeof fetch = async (_url, init) => {
    assert.deepStrictEqual(
      new Headers(init?.headers).get("authorization"),
      "Bearer test",
    );
    const body = JSON.parse(String(init?.body));
    calls.push(body);
    assert.deepStrictEqual(body.storageProvider, "telegram");
    assert.deepStrictEqual(body.schemaVersion, 2);
    assert.deepStrictEqual(body.itemIdentifier, "ClashNyanpasu");
    assert.deepStrictEqual(
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
  assert.deepStrictEqual(calls.length, 0);
  await registerTelegramInventory([target], records, "test", fetcher);
  assert.deepStrictEqual(calls.length, 1);
  await assert.rejects(() =>
      registerTelegramInventory(
        [target],
        records,
        "test",
        async () =>
          Response.json({ error: "Migration missing" }, { status: 500 }),
      ), { message: /HTTP 500/ });
  await assert.rejects(() =>
      registerTelegramInventory(
        [target],
        records,
        "test",
        async () => new Response("<html>fallback</html>"),
      ), Error);
});
