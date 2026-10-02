import { assertEquals } from "jsr:@std/assert@1";
import { inspectUploadReports } from "./verify-upload-reports.ts";

Deno.test("archive upload verification reports missing platform artifacts", () => {
  const issues = inspectUploadReports({});
  assertEquals(issues.length, 8);
  assertEquals(
    issues[0],
    "upload-diagnostics-windows-x86_64-standard: report is missing",
  );
});

Deno.test("archive upload verification reports failed and malformed uploads", () => {
  const issues = inspectUploadReports({
    "upload-diagnostics-windows-x86_64-standard": {
      status: "failure",
      failures: [{ message: "HTTP 503" }],
    },
    "upload-diagnostics-windows-x86_64-fixed-webview": {
      status: "success",
      failures: [{ message: "unexpected failure" }],
    },
    "upload-diagnostics-windows-aarch64-standard": null,
  });
  assertEquals(issues, [
    "upload-diagnostics-windows-x86_64-standard: status is failure",
    "upload-diagnostics-windows-x86_64-standard: 1 upload failure(s)",
    "upload-diagnostics-windows-x86_64-fixed-webview: 1 upload failure(s)",
    "upload-diagnostics-windows-aarch64-standard: report is missing",
    ...[
      "upload-diagnostics-windows-aarch64-fixed-webview",
      "upload-diagnostics-linux-x86_64",
      "upload-diagnostics-linux-aarch64",
      "upload-diagnostics-macos-amd64",
      "upload-diagnostics-macos-aarch64",
    ].map((name) => `${name}: report is missing`),
  ]);
});

Deno.test("archive upload verification accepts all successful reports", () => {
  const reports = Object.fromEntries(
    [
      "upload-diagnostics-windows-x86_64-standard",
      "upload-diagnostics-windows-x86_64-fixed-webview",
      "upload-diagnostics-windows-aarch64-standard",
      "upload-diagnostics-windows-aarch64-fixed-webview",
      "upload-diagnostics-linux-x86_64",
      "upload-diagnostics-linux-aarch64",
      "upload-diagnostics-macos-amd64",
      "upload-diagnostics-macos-aarch64",
    ].map((name) => [name, { status: "success", failures: [] }]),
  );
  assertEquals(inspectUploadReports(reports), []);
});
