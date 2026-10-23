import assert from "node:assert/strict";
import { test } from "node:test";
import { probeArchiveApi, storageConfiguration } from "./storage-preflight.ts";

test("disabled storage needs no credentials; enabled storage rejects missing username immediately", () => {
  assert.deepStrictEqual(storageConfiguration(() => undefined), {
    sourceforge: null,
    archive: null,
    telegram: null,
    token: undefined,
  });
  assert.throws(() =>
      storageConfiguration((key) =>
        key === "SOURCEFORGE_PROJECT" ? "nyanpasu" : undefined
      ), { message: /SOURCEFORGE_USERNAME is required/ });
});

test("archive preflight probes auth and database without registering files", async () => {
  const calls: string[] = [];
  await probeArchiveApi("test-token", async (input, init) => {
    calls.push(String(input));
    assert.deepStrictEqual(
      new Headers(init?.headers).get("authorization"),
      "Bearer test-token",
    );
    if (init?.method === "POST") {
      assert.deepStrictEqual(init.body, "{}");
      return Response.json({ error: "Invalid build manifest" }, {
        status: 400,
      });
    }
    assert.deepStrictEqual(init?.body, undefined);
    return Response.json({ error: "Build not found" }, { status: 404 });
  });
  assert.deepStrictEqual(calls, [
    "https://archive.nyanpasu.org/archive/builds",
    "https://archive.nyanpasu.org/archive/builds/storage-preflight",
  ]);
});

test("Telegram preflight requires MTProto credentials and indexing auth without IA credentials", () => {
  const values: Record<string, string> = {
    TELEGRAM_ARCHIVE_CHANNEL: "@ExampleArchive",
    TELEGRAM_API_ID: "123",
    TELEGRAM_API_HASH: "hash",
    TELEGRAM_TOKEN: "bot-token",
    FILE_SERVER_TOKEN: "index-token",
  };
  const configuration = storageConfiguration((key) => values[key]);
  assert.deepStrictEqual(configuration.telegram, "@ExampleArchive");
  assert.deepStrictEqual(configuration.archive, null);
  assert.deepStrictEqual(configuration.token, "index-token");
  assert.throws(() =>
      storageConfiguration((key) =>
        key === "TELEGRAM_API_ID" ? "invalid" : values[key]
      ), { message: /Invalid TELEGRAM_API_ID/ });
  assert.throws(() =>
      storageConfiguration((key) =>
        key === "FILE_SERVER_TOKEN" ? undefined : values[key]
      ), { message: /archive upload token/ });
});

test("archive preflight surfaces authentication and database failures", async () => {
  let calls = 0;
  await assert.rejects(() =>
      probeArchiveApi("test-token", () => {
        calls++;
        return Promise.resolve(
          Response.json({ error: "Unauthorized" }, { status: 401 }),
        );
      }), { message: /HTTP 401/ });
  assert.deepStrictEqual(calls, 1);
  await assert.rejects(() =>
      probeArchiveApi("test-token", (_input, init) =>
        Promise.resolve(
          init?.method === "POST"
            ? Response.json({ error: "Invalid build manifest" }, {
              status: 400,
            })
            : Response.json({
              error: "D1_ERROR: no such table: archive_builds",
            }, { status: 500 }),
        )), { message: /no such table: archive_builds/ });
});

test("archive preflight rejects HTML fallback and unrelated validation endpoints", async () => {
  await assert.rejects(() =>
      probeArchiveApi("test-token", () =>
        Promise.resolve(
          new Response("<html>fallback</html>", { status: 200 }),
        )), { message: /HTTP 200/ });
  await assert.rejects(() =>
      probeArchiveApi("test-token", () =>
        Promise.resolve(
          Response.json({ error: "other endpoint" }, { status: 400 }),
        )), { message: /Unexpected Archive API/ });
});
