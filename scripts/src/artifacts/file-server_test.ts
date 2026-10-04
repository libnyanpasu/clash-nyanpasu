import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  CHUNK_RETRY_ATTEMPTS,
  initUploadSession,
  performChunkedUpload,
  uploadAllFilesWithResults,
  uploadChunk,
} from "./file-server.ts";

Deno.test("upload batch continues after a file exhausts retries", async () => {
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

  assertEquals([...new Set(attempted)].sort(), [...files].sort());
  assertEquals(
    attempted.filter((file) => file === "unavailable.deb").length,
    CHUNK_RETRY_ATTEMPTS,
  );
  assertEquals(
    outcome.results.map(({ fileName }) => fileName).sort(),
    ["first.dmg", "last.rpm", "next.AppImage"].sort(),
  );
  assertEquals(outcome.failures.length, 1);
  assertEquals(outcome.failures[0].fileName, "unavailable.deb");
  assertEquals(
    outcome.failures[0].message.includes("HTTP 503"),
    true,
    outcome.failures[0].message,
  );
});

Deno.test("archive upload init preserves the release category path", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    assertEquals(String(input), "https://archive.nyanpasu.org/upload/init");
    assertEquals(init?.headers, {
      "x-authorization": "test-token",
      "Content-Type": "application/json",
    });
    assertEquals(JSON.parse(String(init?.body)), {
      filename: "installer.exe",
      fileSize: 1024,
      mimeType: null,
      chunkMultiplier: 160,
      folderPath: "release/v2.0.0-beta.1",
    });
    return Response.json({ uploadId: "test-session", chunkSize: 52428800 });
  };
  try {
    assertEquals(
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

Deno.test("archive init failure preserves the server status and error detail", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = () =>
    Promise.resolve(Response.json({ error: "Unauthorized" }, { status: 401 }));
  try {
    await assertRejects(
      () => initUploadSession("installer.exe", 1024, null, "test-token"),
      Error,
      'upload init failed: 401  - {"error":"Unauthorized"}',
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

for (const stage of ["init", "chunk"] as const) {
  Deno.test(`archive ${stage} quota failure bypasses all retry layers`, async () => {
    const originalFetch = globalThis.fetch;
    const filePath = await Deno.makeTempFile();
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
      await Deno.writeFile(filePath, new Uint8Array([1]));
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
      assertEquals(attempts, 1);
      assertEquals(outcome.results, []);
      assertEquals(outcome.failures.length, 1);
      assertEquals(
        outcome.failures[0].message.includes("quotaLimitReached"),
        true,
      );
      assertEquals(
        outcome.failures[0].message.includes("Free archive storage"),
        true,
      );
    } finally {
      globalThis.fetch = originalFetch;
      await Deno.remove(filePath);
    }
  });
}

for (
  const response of [
    { status: 507, body: "Insufficient Storage" },
    { status: 500, body: '{"error":{"code":"quotaLimitReached"}}' },
  ]
) {
  Deno.test(`archive quota classification accepts ${response.status}: ${response.body}`, async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = () =>
      Promise.resolve(new Response(response.body, { status: response.status }));
    try {
      const error = await assertRejects(
        () => initUploadSession("test.dmg", 1, null, "test-token"),
        Error,
        "Free archive storage",
      );
      assertEquals(error.name, "ArchiveQuotaError");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
}
