import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CHUNK_RETRY_ATTEMPTS,
  initUploadSession,
  performChunkedUpload,
  uploadAllFilesWithResults,
  uploadChunk,
} from "./file-server.ts";
import { rm, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function makeTempFile(): Promise<string> {
  const path = join(tmpdir(), `nyanpasu-test-${randomUUID()}`);
  await writeFile(path, "");
  return path;
}

test("upload batch continues after a file exhausts retries", async () => {
  const files = ["first.dmg", "unavailable.deb", "last.rpm", "next.AppImage"];
  const attempted: string[] = [];
  const outcome = await uploadAllFilesWithResults(
    files,
    "test-token",
    "nightly/test",
    async (filePath) => {
      attempted.push(filePath);
      if (filePath === "unavailable.deb") {
        throw new Error("HTTP 503 from archive");
      }
      return {
        fileName: filePath,
        downloadUrl: `https://archive.example/${filePath}`,
      };
    },
  );

  assert.deepStrictEqual([...new Set(attempted)].sort(), [...files].sort());
  assert.deepStrictEqual(
    attempted.filter((file) => file === "unavailable.deb").length,
    CHUNK_RETRY_ATTEMPTS,
  );
  assert.deepStrictEqual(
    outcome.results.map(({ fileName }) => fileName).sort(),
    ["first.dmg", "last.rpm", "next.AppImage"].sort(),
  );
  assert.deepStrictEqual(outcome.failures.length, 1);
  assert.deepStrictEqual(outcome.failures[0].fileName, "unavailable.deb");
  assert.deepStrictEqual(
    outcome.failures[0].message.includes("HTTP 503"),
    true,
    outcome.failures[0].message,
  );
});

test("archive upload init preserves the release category path", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    assert.deepStrictEqual(String(input), "https://archive.nyanpasu.org/upload/init");
    assert.deepStrictEqual(init?.headers, {
      "x-authorization": "test-token",
      "Content-Type": "application/json",
    });
    assert.deepStrictEqual(JSON.parse(String(init?.body)), {
      filename: "installer.exe",
      fileSize: 1024,
      mimeType: null,
      chunkMultiplier: 160,
      folderPath: "release/v2.0.0-beta.1",
    });
    return Response.json({ uploadId: "test-session", chunkSize: 52428800 });
  };
  try {
    assert.deepStrictEqual(
      await initUploadSession(
        "installer.exe",
        1024,
        null,
        "test-token",
        "release/v2.0.0-beta.1",
      ),
      { uploadId: "test-session", chunkSize: 52428800 },
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("archive init failure preserves the server status and error detail", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = () =>
    Promise.resolve(Response.json({ error: "Unauthorized" }, { status: 401 }));
  try {
    await assert.rejects(() => initUploadSession("installer.exe", 1024, null, "test-token"), { message: /upload init failed: 401 {2}- \{"error":"Unauthorized"\}/ });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

for (const stage of ["init", "chunk"] as const) {
  test(`archive ${stage} quota failure bypasses all retry layers`, async () => {
    const originalFetch = globalThis.fetch;
    const filePath = await makeTempFile();
    let attempts = 0;
    globalThis.fetch = () => {
      attempts++;
      return Promise.resolve(Response.json({
        error: "upload failed",
        detail: `Failed to create upload session: ${
          JSON.stringify({ error: { code: "quotaLimitReached" } })
        }`,
      }, { status: 500 }));
    };
    try {
      await writeFile(filePath, new Uint8Array([1]));
      const outcome = await uploadAllFilesWithResults(
        [filePath],
        "test-token",
        "nightly/test",
        async () => {
          if (stage === "init") {
            await initUploadSession("test.dmg", 1, null, "test-token");
          } else {
            await performChunkedUpload({
              filePath,
              fileSize: 1,
              uploadId: "test-session",
              chunkSize: 1,
              label: "test.dmg",
              uploadChunkFn: (chunk, start, end, total) =>
                uploadChunk(
                  "test-session",
                  chunk,
                  start,
                  end,
                  total,
                  "test-token",
                ),
            });
          }
          throw new Error("quota response must fail");
        },
      );
      assert.deepStrictEqual(attempts, 1);
      assert.deepStrictEqual(outcome.results, []);
      assert.deepStrictEqual(outcome.failures.length, 1);
      assert.deepStrictEqual(
        outcome.failures[0].message.includes("quotaLimitReached"),
        true,
      );
      assert.deepStrictEqual(
        outcome.failures[0].message.includes("Free archive storage"),
        true,
      );
    } finally {
      globalThis.fetch = originalFetch;
      await rm(filePath);
    }
  });
}

for (
  const response of [
    { status: 507, body: "Insufficient Storage" },
    { status: 500, body: '{"error":{"code":"quotaLimitReached"}}' },
  ]
) {
  test(`archive quota classification accepts ${response.status}: ${response.body}`, async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = () =>
      Promise.resolve(new Response(response.body, { status: response.status }));
    try {
      await assert.rejects(
        () => initUploadSession("test.dmg", 1, null, "test-token"),
        (error: Error) =>
          /Free archive storage/.test(error.message) &&
          error.name === "ArchiveQuotaError",
      );
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
}
