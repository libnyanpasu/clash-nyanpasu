import { request as httpRequest } from "node:http";
import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  allowedIaUploadUrl,
  digestFile,
  registerBuild,
  runArchivePublish,
  streamUpload,
  uploadFileRequest,
} from "./internet-archive-upload.ts";

for (
  const [status, detail] of [
    [500, "IA_ITEM_PREFIX and IA_UPLOADER must be configured"],
    [500, "D1_ERROR: no such table: archive_builds"],
    [200, "<html>application fallback</html>"],
  ] as const
) {
  Deno.test(`registration surfaces permanent response ${status}: ${detail}`, async () => {
    const originalFetch = globalThis.fetch;
    let calls = 0;
    try {
      globalThis.fetch = () => {
        calls++;
        return Promise.resolve(
          new Response(detail, {
            status,
            headers: {
              "content-type": status === 200 ? "text/html" : "application/json",
            },
          }),
        );
      };
      await assertRejects(
        () =>
          registerBuild("https://archive.nyanpasu.org", "test-token", {
            schemaVersion: 1,
            buildId: "test-build",
            itemIdentifier: "test-item",
            channel: "nightly",
            commit: "a".repeat(40),
            tag: null,
            folderPath: "nightly/test",
            artifacts: [],
          }),
        Error,
        detail,
      );
      assertEquals(calls, 1);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
}

Deno.test("registration failure retains HTTP detail and the actual attempt count", async () => {
  const originalFetch = globalThis.fetch;
  let calls = 0;
  try {
    globalThis.fetch = () => {
      calls++;
      return Promise.resolve(
        Response.json({
          error: "Invalid build manifest",
          detail: "too many artifacts",
        }, { status: 400 }),
      );
    };
    await assertRejects(
      () =>
        registerBuild("https://archive.nyanpasu.org", "test-token", {
          schemaVersion: 1,
          buildId: "test-build",
          itemIdentifier: "test-item",
          channel: "nightly",
          commit: "a".repeat(40),
          tag: null,
          folderPath: "nightly/test",
          artifacts: [],
        }),
      Error,
      'failed after 1 attempt(s): Archive API returned HTTP 400: {"error":"Invalid build manifest","detail":"too many artifacts"}',
    );
    assertEquals(calls, 1);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

Deno.test("register-only validates and registers original bytes without IA credentials or uploads", async () => {
  const originalFetch = globalThis.fetch;
  const keys = [
    "ARCHIVE_UPLOAD_TOKEN",
    "FILE_SERVER_TOKEN",
    "UPLOAD_TOKEN",
    "IA_ACCESS_KEY",
    "IA_SECRET_KEY",
    "IA_ITEM_PREFIX",
  ];
  const previous = keys.map((key) => Deno.env.get(key));
  const directory = await Deno.makeTempDir();
  let calls = 0;
  try {
    for (const key of keys) Deno.env.delete(key);
    Deno.env.set("UPLOAD_TOKEN", "test-token");
    Deno.env.set("IA_ITEM_PREFIX", "test");
    const filePath = `${directory}/package.zip`;
    await Deno.writeTextFile(filePath, "package");
    const manifest = {
      schemaVersion: 1,
      buildId: "test-build",
      itemIdentifier: "test-item",
      channel: "nightly",
      commit: "a".repeat(40),
      tag: null,
      target: "windows-x86_64",
      folderPath: "nightly/test",
      publishedAt: "2026-10-05T01:02:03.000Z",
      artifacts: [{
        path: filePath,
        fileName: "package.zip",
        ...await digestFile(filePath),
      }],
    };
    await Deno.writeTextFile(
      `${directory}/manifest.json`,
      JSON.stringify(manifest),
    );
    globalThis.fetch = (input) => {
      calls++;
      assertEquals(
        String(input),
        "https://archive.nyanpasu.org/archive/builds",
      );
      return Promise.resolve(Response.json({
        ...manifest,
        status: "pending",
        diagnostics: [],
        artifacts: manifest.artifacts.map((artifact) => ({
          ...artifact,
          storageKey: artifact.fileName,
          downloadUrl: "https://archive.nyanpasu.org/bin/test-file",
        })),
      }));
    };
    assertEquals(
      await runArchivePublish([
        "--register-only",
        "--manifest",
        `${directory}/manifest.json`,
        "--server",
        "https://archive.nyanpasu.org",
        "--report",
        `${directory}/report.json`,
      ]),
      0,
    );
    const report = JSON.parse(
      await Deno.readTextFile(`${directory}/report.json`),
    );
    assertEquals(report.registrationOnly, true);
    assertEquals(report.uploads, []);
    assertEquals(calls, 1);
  } finally {
    globalThis.fetch = originalFetch;
    keys.forEach((key, index) =>
      previous[index] === undefined
        ? Deno.env.delete(key)
        : Deno.env.set(key, previous[index]!)
    );
    await Deno.remove(directory, { recursive: true });
  }
});

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
  let fetchCount = 0;
  try {
    await Deno.writeTextFile(path, "stream payload");
    const upload = async (
      filePath: string,
      _url: URL,
      headers: Record<string, string>,
    ) => {
      fetchCount++;
      const init = { headers, body: (await Deno.open(filePath)).readable };
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
          upload,
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

Deno.test("IA streaming PUT sends Content-Length on the wire", async () => {
  const path = await Deno.makeTempFile();
  const payload = new Uint8Array(2 * 1024 * 1024 + 17).fill(97);
  const controller = new AbortController();
  let length: string | null = null;
  let transferEncoding: string | null = null;
  let received = new Uint8Array();
  const server = Deno.serve({
    hostname: "127.0.0.1",
    port: 0,
    signal: controller.signal,
    onListen() {},
  }, async (request) => {
    length = request.headers.get("content-length");
    transferEncoding = request.headers.get("transfer-encoding");
    received = new Uint8Array(await request.arrayBuffer());
    return new Response(null, { status: length ? 201 : 411 });
  });
  try {
    await Deno.writeFile(path, payload);
    await streamUpload(
      path,
      new URL("https://s3.us.archive.org/test-item/file.zip"),
      payload.length,
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
      (filePath, _url, headers, signal) =>
        uploadFileRequest(
          filePath,
          new URL(`http://127.0.0.1:${server.addr.port}/item/file`),
          headers,
          signal,
          httpRequest,
        ),
    );
    assertEquals(length, String(payload.length));
    assertEquals(transferEncoding, null);
    assertEquals(received, payload);
  } finally {
    controller.abort();
    await server.finished;
    await Deno.remove(path);
  }
});

Deno.test("IA redirected PUT reopens the file and retains length and metadata", async () => {
  const path = await Deno.makeTempFile();
  const controller = new AbortController();
  const requests: {
    length: string | null;
    payload: string;
    bucket: string | null;
  }[] = [];
  const server = Deno.serve({
    hostname: "127.0.0.1",
    port: 0,
    signal: controller.signal,
    onListen() {},
  }, async (request) => {
    requests.push({
      length: request.headers.get("content-length"),
      payload: await request.text(),
      bucket: request.headers.get("x-archive-auto-make-bucket"),
    });
    return requests.length === 1
      ? new Response(null, {
        status: 307,
        headers: {
          location: "https://test-item.s3.us.archive.org/test-item/file.zip",
        },
      })
      : new Response(null, { status: 204 });
  });
  try {
    await Deno.writeTextFile(path, "redirect payload");
    await streamUpload(
      path,
      new URL("https://s3.us.archive.org/test-item/file.zip"),
      16,
      "test-access",
      "test-secret",
      true,
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
      (filePath, _url, headers, signal) =>
        uploadFileRequest(
          filePath,
          new URL(`http://127.0.0.1:${server.addr.port}/item/file`),
          headers,
          signal,
          httpRequest,
        ),
    );
    assertEquals(
      requests,
      Array(2).fill({ length: "16", payload: "redirect payload", bucket: "1" }),
    );
  } finally {
    controller.abort();
    await server.finished;
    await Deno.remove(path);
  }
});

Deno.test("IA HTTP transport retains error bodies and sends empty files with length zero", async () => {
  const path = await Deno.makeTempFile();
  const controller = new AbortController();
  let length: string | null = null;
  const server = Deno.serve({
    hostname: "127.0.0.1",
    port: 0,
    signal: controller.signal,
    onListen() {},
  }, async (request) => {
    length = request.headers.get("content-length");
    await request.arrayBuffer();
    return new Response("permission denied", { status: 403 });
  });
  try {
    const response = await uploadFileRequest(
      path,
      new URL(`http://127.0.0.1:${server.addr.port}/item/file`),
      { "content-length": "0" },
      controller.signal,
      httpRequest,
    );
    assertEquals(length, "0");
    assertEquals(response.status, 403);
    assertEquals(await response.text(), "permission denied");
  } finally {
    controller.abort();
    await server.finished;
    await Deno.remove(path);
  }
});
