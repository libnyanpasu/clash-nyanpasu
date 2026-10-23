import assert from "node:assert/strict";
import { test } from "node:test";
import { createHash } from "node:crypto";
import {
  collectSourceforgeReports,
  type SourceforgeUploadReport,
  validateSourceforgeReports,
  verifySourceforgeMirror,
} from "./verify-sourceforge-mirror.ts";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const bytes = new TextEncoder().encode("signed installer bytes");
function report(
  target: string,
  fileName = `${target}+build.zip`,
): SourceforgeUploadReport {
  return {
    schemaVersion: 1,
    status: "uploaded",
    project: "future-project",
    channel: "nightly",
    buildId: "123-1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    remotePath: "nightly/123-1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    target,
    artifacts: [{
      fileName,
      fileSize: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      url:
        `https://downloads.sourceforge.net/project/future-project/nightly/123-1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/${
          encodeURIComponent(fileName)
        }`,
    }],
  };
}

test("complete same-build reports stream through public SHA-256 verification", async () => {
  const reports = [
    { ...report("linux"), publishedAt: "2026-10-04T01:02:03.000Z" },
    {
      ...report("windows", "Clash Nyanpasu+windows.zip"),
      publishedAt: "2026-10-04T01:02:03.000Z",
    },
  ];
  const requests: string[] = [];
  const result = await verifySourceforgeMirror(
    reports,
    ["linux", "windows"],
    "nightly",
    {
      fetcher: ((input) => {
        requests.push(String(input));
        return Promise.resolve(
          new Response(bytes, {
            headers: { "content-type": "application/zip" },
          }),
        );
      }) as typeof fetch,
      attempts: 1,
    },
  );
  assert.deepStrictEqual(requests.length, 2);
  assert.deepStrictEqual(result.publishedAt, "2026-10-04T01:02:03.000Z");
  assert.deepStrictEqual(
    result.assets["Clash Nyanpasu+windows.zip"].sha256,
    reports[1].artifacts[0].sha256,
  );
  assert.deepStrictEqual(
    result.buildId,
    "123-1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  );
});

test("public mirror verification bounds transfers and drains after a hash failure", async () => {
  const reports = Array.from({ length: 6 }, (_, i) => report(`target-${i}`));
  const release = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  let active = 0;
  let maximum = 0;
  let requests = 0;
  const verification = verifySourceforgeMirror(
    reports,
    reports.map((item) => item.target),
    "nightly",
    {
      fetcher: (async (input) => {
        requests++;
        active++;
        maximum = Math.max(maximum, active);
        if (requests === 4) started.resolve();
        await release.promise;
        active--;
        return new Response(
          String(input).includes("target-0")
            ? new Uint8Array(bytes.length)
            : bytes,
          { headers: { "content-type": "application/zip" } },
        );
      }) as typeof fetch,
      attempts: 1,
    },
  );
  const rejected = assert.rejects(() => verification);
  await started.promise;
  assert.deepStrictEqual(requests, 4);
  release.resolve();
  await rejected;
  assert.deepStrictEqual(maximum, 4);
  assert.deepStrictEqual(requests, 6);
});

test("reject missing, duplicate, unexpected and empty target reports", () => {
  for (
    const reports of [
      [],
      [report("linux")],
      [report("linux"), report("linux")],
      [report("linux"), report("windows"), report("other")],
      [report("linux"), { ...report("windows"), artifacts: [] }],
    ]
  ) {
    assert.deepStrictEqual(
      validateSourceforgeReports(reports, ["linux", "windows"]).length > 0,
      true,
    );
  }
});

test("reject mismatched builds, traversal and non-canonical public URLs before fetch", async () => {
  for (
    const invalid of [
      { ...report("linux"), buildId: "other" },
      { ...report("linux"), project: "other" },
      { ...report("linux"), channel: "release" },
      { ...report("linux"), status: "failure" },
      { ...report("linux"), publishedAt: "invalid" },
      {
        ...report("linux"),
        artifacts: [{
          ...report("linux").artifacts[0],
          fileName: "../app.zip",
        }],
      },
      {
        ...report("linux"),
        artifacts: [{
          ...report("linux").artifacts[0],
          url: "https://example.com/app.zip",
        }],
      },
      {
        ...report("linux"),
        artifacts: [{
          ...report("linux").artifacts[0],
          url: report("linux").artifacts[0].url + "?anything=1",
        }],
      },
    ]
  ) {
    await assert.rejects(() =>
      verifySourceforgeMirror([invalid], ["linux"], "nightly", {
        fetcher: (() => {
          throw new Error("must not perform network IO");
        }) as typeof fetch,
      }));
  }
  assert.deepStrictEqual(
    validateSourceforgeReports([report("linux"), {
      ...report("windows"),
      buildId: "other",
      remotePath: "nightly/other",
    }], ["linux", "windows"]).length > 0,
    true,
  );
  assert.deepStrictEqual(
    validateSourceforgeReports([report("linux"), {
      ...report("windows"),
      publishedAt: "2026-10-04T01:02:03.000Z",
    }], ["linux", "windows"]).length > 0,
    true,
  );
});

test("transient mirror delay retries but HTML and wrong bytes never publish", async () => {
  let attempts = 0;
  let waits = 0;
  await verifySourceforgeMirror([report("linux")], ["linux"], "nightly", {
    fetcher: (() =>
      Promise.resolve(
        ++attempts === 1
          ? new Response("not replicated", { status: 404 })
          : new Response(bytes),
      )) as typeof fetch,
    wait: () => {
      waits++;
      return Promise.resolve();
    },
    attempts: 2,
  });
  assert.deepStrictEqual([attempts, waits], [2, 1]);
  for (
    const response of [
      () =>
        new Response("<html>selection</html>", {
          headers: { "content-type": "text/html" },
        }),
      () => new Response(new Uint8Array(bytes.length)),
      () => new Response(bytes.subarray(0, bytes.length - 1)),
      () => new Response(new Uint8Array(bytes.length + 1)),
    ]
  ) {
    await assert.rejects(() =>
        verifySourceforgeMirror([report("linux")], ["linux"], "nightly", {
          fetcher: (() => Promise.resolve(response())) as typeof fetch,
          attempts: 1,
        }), { message: /verification failed/ });
  }
});

test("collect reports nested under separate platform artifact directories", async () => {
  const root = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  try {
    await mkdir(`${root}/a/nested`, { recursive: true });
    await writeFile(
      `${root}/a/nested/sourceforge-report.json`,
      JSON.stringify(report("linux")),
    );
    await writeFile(`${root}/irrelevant.json`, "{}");
    assert.deepStrictEqual(await collectSourceforgeReports(root), [report("linux")]);
  } finally {
    await rm(root, { recursive: true });
  }
});
