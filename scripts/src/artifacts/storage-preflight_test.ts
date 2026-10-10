import { assertEquals, assertRejects, assertThrows } from "jsr:@std/assert@1";
import { probeArchiveApi, storageConfiguration } from "./storage-preflight.ts";

Deno.test("disabled storage needs no credentials; enabled storage rejects missing username immediately", () => {
  assertEquals(storageConfiguration(() => undefined), {
    sourceforge: null,
    archive: null,
    telegram: null,
    token: undefined,
  });
  assertThrows(
    () =>
      storageConfiguration((key) =>
        key === "SOURCEFORGE_PROJECT" ? "nyanpasu" : undefined
      ),
    Error,
    "SOURCEFORGE_USERNAME is required",
  );
});

Deno.test("archive preflight probes auth and database without registering files", async () => {
  const calls: string[] = [];
  await probeArchiveApi("test-token", async (input, init) => {
    calls.push(String(input));
    assertEquals(
      new Headers(init?.headers).get("authorization"),
      "Bearer test-token",
    );
    if (init?.method === "POST") {
      assertEquals(init.body, "{}");
      return Response.json({ error: "Invalid build manifest" }, {
        status: 400,
      });
    }
    assertEquals(init?.body, undefined);
    return Response.json({ error: "Build not found" }, { status: 404 });
  });
  assertEquals(calls, [
    "https://archive.nyanpasu.org/archive/builds",
    "https://archive.nyanpasu.org/archive/builds/storage-preflight",
  ]);
});

Deno.test("Telegram preflight requires MTProto credentials and indexing auth without IA credentials", () => {
  const values: Record<string, string> = {
    TELEGRAM_ARCHIVE_CHANNEL: "@ExampleArchive",
    TELEGRAM_API_ID: "123",
    TELEGRAM_API_HASH: "hash",
    TELEGRAM_TOKEN: "bot-token",
    FILE_SERVER_TOKEN: "index-token",
  };
  const configuration = storageConfiguration((key) => values[key]);
  assertEquals(configuration.telegram, "@ExampleArchive");
  assertEquals(configuration.archive, null);
  assertEquals(configuration.token, "index-token");
  assertThrows(
    () =>
      storageConfiguration((key) =>
        key === "TELEGRAM_API_ID" ? "invalid" : values[key]
      ),
    Error,
    "Invalid TELEGRAM_API_ID",
  );
  assertThrows(
    () =>
      storageConfiguration((key) =>
        key === "FILE_SERVER_TOKEN" ? undefined : values[key]
      ),
    Error,
    "archive upload token",
  );
});

Deno.test("archive preflight surfaces authentication and database failures", async () => {
  let calls = 0;
  await assertRejects(
    () =>
      probeArchiveApi("test-token", () => {
        calls++;
        return Promise.resolve(
          Response.json({ error: "Unauthorized" }, { status: 401 }),
        );
      }),
    Error,
    "HTTP 401",
  );
  assertEquals(calls, 1);
  await assertRejects(
    () =>
      probeArchiveApi("test-token", (_input, init) =>
        Promise.resolve(
          init?.method === "POST"
            ? Response.json({ error: "Invalid build manifest" }, {
              status: 400,
            })
            : Response.json({
              error: "D1_ERROR: no such table: archive_builds",
            }, { status: 500 }),
        )),
    Error,
    "no such table: archive_builds",
  );
});

Deno.test("archive preflight rejects HTML fallback and unrelated validation endpoints", async () => {
  await assertRejects(
    () =>
      probeArchiveApi("test-token", () =>
        Promise.resolve(
          new Response("<html>fallback</html>", { status: 200 }),
        )),
    Error,
    "HTTP 200",
  );
  await assertRejects(
    () =>
      probeArchiveApi("test-token", () =>
        Promise.resolve(
          Response.json({ error: "other endpoint" }, { status: 400 }),
        )),
    Error,
    "Unexpected Archive API",
  );
});
