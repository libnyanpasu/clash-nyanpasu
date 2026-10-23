import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import * as path from "node:path";
import { globby } from "globby";
import { uploadAllFilesWithResults, type UploadResult } from "./file-server.ts";
import { consola } from "../shared/logger.ts";
import { appendFile, writeFile } from "node:fs/promises";

interface UploadReport {
  status: "success" | "failure";
  folderPath: string | null;
  patterns: string[];
  matchedFiles: string[];
  uploadedFiles: UploadResult[];
  failures: Array<{ fileName: string; message: string; stack?: string }>;
}

function errorDetails(error: unknown): { message: string; stack?: string } {
  if (error instanceof Error) {
    return {
      message: `${error.name}: ${error.message}`,
      ...(error.stack ? { stack: error.stack } : {}),
    };
  }
  return { message: String(error) };
}

const patterns = process.argv.slice(2);
const folderPath = process.env["FOLDER_PATH"] ?? null;
const report: UploadReport = {
  status: "failure",
  folderPath,
  patterns,
  matchedFiles: [],
  uploadedFiles: [],
  failures: [],
};

try {
  if (patterns.length === 0) {
    throw new Error("No file patterns provided as arguments");
  }

  const token = process.env["FILE_SERVER_TOKEN"];
  if (!token) {
    throw new Error("FILE_SERVER_TOKEN is required");
  }
  if (!folderPath) {
    throw new Error("FOLDER_PATH is required");
  }

  consola.info(`Searching for files matching: ${patterns.join(", ")}`);
  const files = await globby(patterns, { cwd: WORKSPACE_ROOT, absolute: true });
  report.matchedFiles = files.map((file) =>
    path.relative(WORKSPACE_ROOT, file)
  );

  consola.info(`Found ${files.length} files:`);
  for (const file of files) {
    consola.info(`  ${path.basename(file)}`);
  }
  if (files.length === 0) {
    throw new Error(
      `No files matched the upload patterns: ${patterns.join(", ")}`,
    );
  }

  const outcome = await uploadAllFilesWithResults(files, token, folderPath);
  report.uploadedFiles = outcome.results;
  report.failures = outcome.failures;
  if (outcome.failures.length > 0) {
    throw new AggregateError(
      outcome.failures.map(({ fileName, message }) =>
        new Error(`${fileName}: ${message}`)
      ),
      `Failed to upload ${outcome.failures.length} of ${files.length} files`,
    );
  }
  report.status = "success";
} catch (error) {
  const details = errorDetails(error);
  if (report.failures.length === 0) {
    report.failures.push({ fileName: "(upload setup)", ...details });
  }
  consola.error(details.message);
  for (const failure of report.failures) {
    consola.error(`${failure.fileName}: ${failure.message}`);
  }
}

const reportPath = path.join(WORKSPACE_ROOT, "upload-report.json");
await writeFile(reportPath, JSON.stringify(report, null, 2));

const summaryPath = process.env["GITHUB_STEP_SUMMARY"];
if (summaryPath) {
  const lines = [
    `## Archive upload: ${report.status}`,
    "",
    `Folder: \`${report.folderPath ?? "(unset)"}\``,
    `Matched files: ${report.matchedFiles.length}`,
    `Uploaded files: ${report.uploadedFiles.length}`,
  ];
  if (report.failures.length > 0) {
    lines.push("", "Failures:");
    for (const failure of report.failures) {
      lines.push(`- \`${failure.fileName}\`: ${failure.message}`);
    }
  }
  await appendFile(summaryPath, `${lines.join("\n")}\n\n`);
}

if (report.status === "failure") {
  process.exit(1);
}

consola.success(
  `Upload complete. ${report.uploadedFiles.length} files uploaded. Report written to ${reportPath}`,
);
