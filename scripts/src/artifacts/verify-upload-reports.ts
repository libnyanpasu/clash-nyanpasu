import * as path from "jsr:@std/path";

interface UploadReport {
  status?: unknown;
  failures?: unknown;
}

const EXPECTED_REPORTS = [
  "upload-diagnostics-windows-x86_64-standard",
  "upload-diagnostics-windows-x86_64-fixed-webview",
  "upload-diagnostics-windows-aarch64-standard",
  "upload-diagnostics-windows-aarch64-fixed-webview",
  "upload-diagnostics-linux-x86_64",
  "upload-diagnostics-linux-aarch64",
  "upload-diagnostics-macos-amd64",
  "upload-diagnostics-macos-aarch64",
];

export function inspectUploadReports(
  reports: Record<string, unknown>,
): string[] {
  const issues: string[] = [];
  for (const name of EXPECTED_REPORTS) {
    const report = reports[name];
    if (!report || typeof report !== "object" || Array.isArray(report)) {
      issues.push(`${name}: report is missing`);
      continue;
    }
    const uploadReport = report as UploadReport;
    if (uploadReport.status !== "success") {
      issues.push(
        `${name}: status is ${String(uploadReport.status ?? "missing")}`,
      );
    }
    if (!Array.isArray(uploadReport.failures)) {
      issues.push(`${name}: failures field is missing or invalid`);
    } else if (uploadReport.failures.length > 0) {
      issues.push(`${name}: ${uploadReport.failures.length} upload failure(s)`);
    }
  }
  return issues;
}

if (import.meta.main) {
  const reportsDir = path.resolve(Deno.args[0] ?? "upload-diagnostics");
  const reports: Record<string, unknown> = {};

  for (const name of EXPECTED_REPORTS) {
    const reportPath = path.join(reportsDir, name, "upload-report.json");
    try {
      reports[name] = JSON.parse(await Deno.readTextFile(reportPath));
    } catch (error) {
      if (!(error instanceof Deno.errors.NotFound)) {
        reports[name] = { status: "invalid", failures: [String(error)] };
      }
    }
  }

  const issues = inspectUploadReports(reports);
  const summaryPath = Deno.env.get("GITHUB_STEP_SUMMARY");
  if (summaryPath) {
    const lines = issues.length === 0
      ? [
        "## Archive upload verification: passed",
        "All expected platform uploads succeeded.",
      ]
      : [
        "## Archive upload verification: failed",
        ...issues.map((issue) => `- ${issue}`),
      ];
    await Deno.writeTextFile(summaryPath, `${lines.join("\n")}\n\n`, {
      append: true,
    });
  }

  if (issues.length > 0) {
    for (const issue of issues) console.error(issue);
    Deno.exit(1);
  }

  console.log("All expected archive uploads succeeded.");
}
