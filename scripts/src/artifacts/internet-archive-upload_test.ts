import { createServer } from "node:http";
import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  allowedIaUploadUrl,
  digestFile,
  registerBuild,
  runArchivePublish,
  streamUpload,
  uploadFileRequest,
  uploadOne,
} from "./internet-archive-upload.ts";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCommand } from "../shared/command.ts";
import { randomUUID } from "node:crypto";
import { createReadStream } from "node:fs";
import { Readable } from "node:stream";

async function makeTempFile(): Promise<string> {
  const path = join(tmpdir(), `nyanpasu-test-${randomUUID()}`);
  await writeFile(path, "");
  return path;
}

async function serve(
  handler: (request: Request) => Response | Promise<Response>,
): Promise<{ port: number; close: () => Promise<void> }> {
  const server = createServer(async (incoming, outgoing) => {
    const request = new Request(`http://127.0.0.1${incoming.url}`, {
      method: incoming.method,
      headers: incoming.headers as Record<string, string>,
      body: incoming.method === "GET" || incoming.method === "HEAD"
        ? undefined
        : Readable.toWeb(incoming) as ReadableStream,
      duplex: "half",
    } as RequestInit);
    const response = await handler(request);
    outgoing.writeHead(
      response.status,
      Object.fromEntries(response.headers),
    );
    outgoing.end(Buffer.from(await response.arrayBuffer()));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as { port: number };
  return {
    port,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.closeAllConnections();
        server.close((error) => error ? reject(error) : resolve());
      }),
  };
}

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
  const previous = keys.map((key) => process.env[key]);
  const directory = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  let calls = 0;
  try {
    for (const key of keys) delete process.env[key];
    process.env["UPLOAD_TOKEN"] = "test-token";
    process.env["IA_ITEM_PREFIX"] = "test";
    const filePath = `${directory}/package.zip`;
    await writeFile(filePath, "package");
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
    await writeFile(`${directory}/manifest.json`, JSON.stringify(manifest));
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
      await readFile(`${directory}/report.json`, "utf8"),
    );
    assertEquals(report.registrationOnly, true);
    assertEquals(report.uploads, []);
    assertEquals(calls, 1);
  } finally {
    globalThis.fetch = originalFetch;
    keys.forEach((key, index) =>
      previous[index] === undefined
        ? delete process.env[key]
        : process.env[key] = previous[index]!
    );
    await rm(directory, { recursive: true });
  }
});

Deno.test("digestFile streams bytes and returns stable SHA-256 and MD5", async () => {
  const path = await makeTempFile();
  try {
    await writeFile(path, "hello");
    assertEquals(await digestFile(path), {
      fileSize: 5,
      sha256:
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      md5: "5d41402abc4b2a76b9719d911017c592",
    });
  } finally {
    await rm(path);
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

  const path = await makeTempFile();
  let fetchCount = 0;
  try {
    await writeFile(path, "stream payload");
    const upload = async (
      filePath: string,
      _url: URL,
      headers: Record<string, string>,
    ) => {
      fetchCount++;
      const init = {
        headers,
        body: Readable.toWeb(createReadStream(filePath)) as ReadableStream,
      };
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
    await rm(path);
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
  const tempDir = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  try {
    const manifestPath = `${tempDir}/manifest.json`;
    const reportPath = `${tempDir}/report.json`;
    const childPath = `${tempDir}/runner.ts`;
    const moduleUrl =
      new URL("./internet-archive-upload.ts", import.meta.url).href;
    await writeFile(
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
    await writeFile(
      childPath,
      `
      import { runArchivePublish } from ${JSON.stringify(moduleUrl)};
      process.exitCode = await runArchivePublish([
        "--manifest", ${JSON.stringify(manifestPath)},
        "--server", "https://archive.nyanpasu.org",
        "--report", ${JSON.stringify(reportPath)},
      ]);
    `,
    );

    const result = await runCommand(process.execPath, {
      args: [childPath],
      clearEnv: true,
      env: {},
      stdout: "piped",
      stderr: "piped",
    });
    assertEquals(result.code, 1);
    const report = JSON.parse(await readFile(reportPath, "utf8"));
    assertEquals(report.status, "failed");
    assertEquals(report.error, "UPLOAD_TOKEN is required");
    assertEquals(report.buildId, "missing-token-fixture");
  } finally {
    await rm(tempDir, { recursive: true });
  }
});

Deno.test("IA streaming PUT sends Content-Length on the wire", async () => {
  const path = await makeTempFile();
  const payload = new Uint8Array(2 * 1024 * 1024 + 17).fill(97);
  const controller = new AbortController();
  let length: string | null = null;
  let transferEncoding: string | null = null;
  let received = new Uint8Array();
  const server = await serve(async (request) => {
    length = request.headers.get("content-length");
    transferEncoding = request.headers.get("transfer-encoding");
    received = new Uint8Array(await request.arrayBuffer());
    return new Response(null, { status: length ? 201 : 411 });
  });
  try {
    await writeFile(path, payload);
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
          new URL(`http://127.0.0.1:${server.port}/item/file`),
          headers,
          signal,
        ),
    );
    assertEquals(length, String(payload.length));
    assertEquals(transferEncoding, null);
    assertEquals(received, payload);
  } finally {
    controller.abort();
    await server.close();
    await rm(path);
  }
});

Deno.test("IA redirected PUT reopens the file and retains length and metadata", async () => {
  const path = await makeTempFile();
  const controller = new AbortController();
  const requests: {
    length: string | null;
    payload: string;
    bucket: string | null;
  }[] = [];
  const server = await serve(async (request) => {
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
    await writeFile(path, "redirect payload");
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
          new URL(`http://127.0.0.1:${server.port}/item/file`),
          headers,
          signal,
        ),
    );
    assertEquals(
      requests,
      Array(2).fill({ length: "16", payload: "redirect payload", bucket: "1" }),
    );
  } finally {
    controller.abort();
    await server.close();
    await rm(path);
  }
});

Deno.test("IA HTTP transport retains error bodies and sends empty files with length zero", async () => {
  const path = await makeTempFile();
  const controller = new AbortController();
  let length: string | null = null;
  const server = await serve(async (request) => {
    length = request.headers.get("content-length");
    await request.arrayBuffer();
    return new Response("permission denied", { status: 403 });
  });
  try {
    const response = await uploadFileRequest(
      path,
      new URL(`http://127.0.0.1:${server.port}/item/file`),
      { "content-length": "0" },
      controller.signal,
    );
    assertEquals(length, "0");
    assertEquals(response.status, 403);
    assertEquals(await response.text(), "permission denied");
  } finally {
    controller.abort();
    await server.close();
    await rm(path);
  }
});

Deno.test("IA early redirect stops sending a large request body", async () => {
  const path = await makeTempFile();
  await writeFile(path, new Uint8Array(32 * 1024 * 1024));
  const server = createServer((_request, response) => {
    response.writeHead(307, {
      location: "https://test-item.s3.us.archive.org/file",
    });
    response.end("redirect");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address() as { port: number };
    const response = await uploadFileRequest(
      path,
      new URL(`http://127.0.0.1:${address.port}/file`),
      { "content-length": String(32 * 1024 * 1024) },
      AbortSignal.timeout(1000),
    );
    assertEquals(response.status, 307);
    assertEquals(await response.text(), "redirect");
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve())
    );
    await rm(path);
  }
});

Deno.test("IA upload abort terminates curl while waiting for a response", async () => {
  const path = await makeTempFile();
  const controller = new AbortController();
  const server = createServer((request, _response) => {
    request.resume();
    request.on("end", () => controller.abort());
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address() as { port: number };
    await assertRejects(() =>
      uploadFileRequest(
        path,
        new URL(`http://127.0.0.1:${address.port}/file`),
        { "content-length": "0" },
        controller.signal,
      )
    );
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve())
    );
    await rm(path);
  }
});

Deno.test("IA low-speed timeout stops a stalled response without the overall deadline", async () => {
  const path = await makeTempFile();
  const server = createServer((request, _response) => request.resume());
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address() as { port: number };
    await assertRejects(
      () =>
        uploadFileRequest(
          path,
          new URL(`http://127.0.0.1:${address.port}/file`),
          { "content-length": "0" },
          AbortSignal.timeout(10_000),
          1000,
        ),
      Error,
      "curl 28",
    );
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve())
    );
    await rm(path);
  }
});

Deno.test("IA retry skips bytes stored before an upload response timed out", async () => {
  const originalFetch = globalThis.fetch;
  const uploader = process.env["IA_UPLOADER"];
  let metadataCalls = 0;
  let uploads = 0;
  try {
    process.env["IA_UPLOADER"] = "test-uploader";
    globalThis.fetch = () => {
      metadataCalls++;
      return Promise.resolve(Response.json(
        metadataCalls === 1 ? {} : {
          metadata: { identifier: "test-item", uploader: "test-uploader" },
          files: [{ name: "file.zip", size: "14", md5: "test-md5" }],
        },
      ));
    };
    const result = await uploadOne(
      "test-item",
      {
        path: "unused",
        fileName: "file.zip",
        fileSize: 14,
        md5: "test-md5",
      },
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
      () => {
        uploads++;
        return Promise.reject(new Error("upload response timed out"));
      },
    );
    assertEquals(result, "skipped");
    assertEquals(uploads, 1);
    assertEquals(metadataCalls, 2);
  } finally {
    globalThis.fetch = originalFetch;
    if (uploader === undefined) delete process.env["IA_UPLOADER"];
    else process.env["IA_UPLOADER"] = uploader;
  }
});
