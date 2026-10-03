import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import { initUploadSession, uploadAllFilesWithResults } from "./file-server.ts";

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
