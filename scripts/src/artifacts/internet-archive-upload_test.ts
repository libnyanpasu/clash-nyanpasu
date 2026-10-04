import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  allowedIaUploadUrl,
  digestFile,
  registerBuild,
  runArchivePublish,
  streamUpload,
} from "./internet-archive-upload.ts";

Deno.test("digestFile streams bytes and returns stable SHA-256 and MD5", async () => {
  const path = await Deno.makeTempFile();
  try {
    await Deno.writeTextFile(path, "hello");
    assertEquals(await digestFile(path), {
      fileSize: 5,
      sha256:
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      md5: "5d41402abc4b2a76b9719d911017c592",
    });
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("IA upload host allowlist and redirect guard prevent credential forwarding", async () => {
  assertEquals(
    allowedIaUploadUrl("https://s3.us.archive.org/item/file.zip"),
    true,
  );
  assertEquals(
    allowedIaUploadUrl("https://item.s3.us.archive.org/item/file.zip", "item"),
    true,
  );
  assertEquals(
    allowedIaUploadUrl("https://evil.example/upload", "item"),
    false,
  );
  assertEquals(
    allowedIaUploadUrl("http://s3.us.archive.org/item/file.zip"),
    false,
  );

  const path = await Deno.makeTempFile();
  const originalFetch = globalThis.fetch;
  let fetchCount = 0;
  try {
    await Deno.writeTextFile(path, "stream payload");
    globalThis.fetch = async (_input, init) => {
      fetchCount++;
      assertEquals(
        new Headers(init?.headers).get("authorization"),
        "LOW test-access:test-secret",
      );
      assertEquals(new Headers(init?.headers).get("content-length"), "14");
      const reader = (init?.body as ReadableStream<Uint8Array>).getReader();
      const chunks: Uint8Array[] = [];
      while (true) {
        const result = await reader.read();
        if (result.done) break;
        chunks.push(result.value);
      }
      const uploaded = new Uint8Array(
        chunks.reduce((total, chunk) => total + chunk.length, 0),
      );
      let offset = 0;
      for (const chunk of chunks) {
        uploaded.set(chunk, offset);
        offset += chunk.length;
      }
      assertEquals(new TextDecoder().decode(uploaded), "stream payload");
      return new Response(null, {
        status: 302,
        headers: { location: "https://evil.example/upload" },
      });
    };

    await assertRejects(
      () =>
        streamUpload(
          path,
          new URL("https://s3.us.archive.org/test-item/file.zip"),
          14,
          "test-access",
          "test-secret",
          false,
          {
            schemaVersion: 1,
            buildId: "test-build",
            itemIdentifier: "test-item",
            channel: "nightly",
            commit: "a".repeat(40),
            tag: null,
            folderPath: "nightly/test",
            artifacts: [],
          },
        ),
      Error,
      "non-IA S3 host",
    );
    assertEquals(
      fetchCount,
      1,
      "redirect destination must be rejected before a second credentialed request",
    );
  } finally {
    globalThis.fetch = originalFetch;
    await Deno.remove(path);
  }
});

Deno.test("registration rejects a shared publication timestamp mismatch before any upload", async () => {
  const originalFetch = globalThis.fetch;
  let fetchCount = 0;
  let requestBody: Record<string, unknown> | undefined;
  try {
    globalThis.fetch = async (input, init) => {
      fetchCount++;
      assertEquals(new URL(String(input)).pathname, "/archive/builds");
      requestBody = JSON.parse(String(init?.body));
      return Response.json({
        buildId: "test-build",
        itemIdentifier: "test-item",
        publishedAt: "2026-10-05T01:02:04.000Z",
        status: "pending",
        diagnostics: [],
        artifacts: [],
      });
    };

    await assertRejects(
      () =>
        registerBuild("https://archive.nyanpasu.org", "test-token", {
          schemaVersion: 1,
          buildId: "test-build",
          itemIdentifier: "test-item",
          channel: "nightly",
          commit: "a".repeat(40),
          publishedAt: "2026-10-05T01:02:03.000Z",
          tag: null,
          folderPath: "nightly/test",
          artifacts: [],
        }),
      Error,
      "publication timestamp mismatch",
    );
    assertEquals(requestBody?.publishedAt, "2026-10-05T01:02:03.000Z");
    assertEquals(
      fetchCount,
      1,
      "registration failure must stop before any IA upload request",
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

Deno.test("runner reports missing credentials with exit status 1 without external requests", async () => {
  const tempDir = await Deno.makeTempDir();
  try {
    const manifestPath = `${tempDir}/manifest.json`;
    const reportPath = `${tempDir}/report.json`;
    const childPath = `${tempDir}/runner.ts`;
    const moduleUrl =
      new URL("./internet-archive-upload.ts", import.meta.url).href;
    await Deno.writeTextFile(
      manifestPath,
      JSON.stringify({
        schemaVersion: 1,
        buildId: "missing-token-fixture",
        itemIdentifier: "fixture-item",
        channel: "nightly",
        commit: "a".repeat(40),
        tag: null,
        folderPath: "nightly/fixture",
        artifacts: [],
      }),
    );
    await Deno.writeTextFile(
      childPath,
      `
      import { runArchivePublish } from ${JSON.stringify(moduleUrl)};
      Deno.exitCode = await runArchivePublish([
        "--manifest", ${JSON.stringify(manifestPath)},
        "--server", "https://archive.nyanpasu.org",
        "--report", ${JSON.stringify(reportPath)},
      ]);
    `,
    );

    const result = await new Deno.Command(Deno.execPath(), {
      args: ["run", "--allow-all", childPath],
      clearEnv: true,
      env: {},
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(result.code, 1);
    const report = JSON.parse(await Deno.readTextFile(reportPath));
    assertEquals(report.status, "failed");
    assertEquals(report.error, "UPLOAD_TOKEN is required");
    assertEquals(report.buildId, "missing-token-fixture");
  } finally {
    await Deno.remove(tempDir, { recursive: true });
  }
});
