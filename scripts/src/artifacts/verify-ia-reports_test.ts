import { assertEquals } from "jsr:@std/assert";
import { validateIaReports } from "./verify-ia-reports.ts";

function report(target: string, status = "pending") {
  return {
    schemaVersion: 1,
    target,
    channel: "nightly",
    publishedAt: "2026-10-04T01:02:03.000Z",
    commit: "a".repeat(40),
    buildId: `123-1-${"a".repeat(40)}-${target}`,
    itemIdentifier: `nyanpasu-123-${target}`,
    folderPath: `nightly/123-1-${"a".repeat(40)}`,
    status,
    artifacts: [{
      fileId: "stable-id",
      fileName: `${target}.zip`,
      fileSize: 42,
      storageKey: `${target}.zip`,
      sha256: "b".repeat(64),
      md5: "c".repeat(32),
      status,
    }],
    uploads: [{ fileName: "artifact-manifest.json", status: "uploaded" }, {
      fileName: `${target}.zip`,
      status: "skipped",
    }],
  };
}
Deno.test("complete uploaded pending ingest is explicit and accepted independently of mirror publication", () => {
  const result = validateIaReports([
    report("linux"),
    report("windows", "ready"),
  ], ["linux", "windows"]);
  assertEquals(result.issues, []);
  assertEquals(result.pending.length, 1);
});
Deno.test("missing, failed, duplicate and mixed-build IA reports fail", () => {
  for (
    const reports of [
      [],
      [report("linux")],
      [report("linux"), report("linux")],
      [report("linux"), report("windows", "failed")],
      [report("linux"), { ...report("windows"), commit: "d".repeat(40) }],
      [report("linux"), {
        ...report("windows"),
        publishedAt: "2026-10-04T02:02:03.000Z",
      }],
    ]
  ) {
    assertEquals(
      validateIaReports(reports, ["linux", "windows"]).issues.length > 0,
      true,
    );
  }
});
Deno.test("pending is rejected if inventory, sanitized manifest or verified hashes are missing", () => {
  const base = report("linux");
  for (
    const invalid of [
      { ...base, artifacts: [] },
      { ...base, publishedAt: undefined },
      { ...base, uploads: [] },
      { ...base, uploads: base.uploads.slice(1) },
      { ...base, uploads: [...base.uploads, base.uploads[1]] },
      { ...base, artifacts: [{ ...base.artifacts[0], sha256: "" }] },
      { ...base, artifacts: [{ ...base.artifacts[0], status: "failed" }] },
    ]
  ) {
    assertEquals(
      validateIaReports([invalid], ["linux"]).issues.length > 0,
      true,
    );
  }
});
