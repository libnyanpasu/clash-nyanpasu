/** Human-readable report and gate output. */
import { consola } from "../shared/logger.ts";
import { type GateResult, type LedgerReport } from "./policy.ts";

export function printHuman(report: LedgerReport): void {
  consola.info("architecture ledger (actor-migration residuals)");
  consola.info(`mode: ${report.mode}`);
  consola.info(`roots: ${report.roots.join(", ")}`);
  consola.info(`scanned rust files: ${report.scannedFiles}`);
  console.log("");

  const rows: Array<[string, number]> = Object.entries(report.metrics).map(
    ([id, metric]) => [id, metric.total],
  );
  const labelWidth = Math.max(...rows.map(([id]) => id.length), 8);
  for (const [id, total] of rows) {
    console.log(`${id.padEnd(labelWidth)}  ${String(total).padStart(6)}`);
  }

  console.log("");
  console.log(
    `bridge_files                 ${
      String(report.bridgeFiles.length).padStart(6)
    }`,
  );
  if (report.bridgeFiles.length > 0) {
    for (const file of report.bridgeFiles) {
      console.log(`  - ${file}`);
    }
  }

  for (const [id, metric] of Object.entries(report.metrics)) {
    const keys = Object.entries(metric.byKey);
    if (keys.length === 0) continue;
    console.log("");
    console.log(`[${id}] breakdown`);
    for (const [key, count] of keys.slice(0, 25)) {
      console.log(`  ${String(count).padStart(5)}  ${key}`);
    }
    if (keys.length > 25) {
      console.log(`  ... ${keys.length - 25} more keys`);
    }
    if (metric.samples.length > 0) {
      console.log(`  samples (up to ${metric.samples.length}):`);
      for (const sample of metric.samples.slice(0, 8)) {
        console.log(
          `    ${sample.file}:${sample.line}: ${sample.detail ?? ""}`,
        );
      }
    }
  }

  if (report.notes.length > 0) {
    console.log("");
    for (const note of report.notes) {
      consola.info(note);
    }
  }
}

export function printGateResult(
  result: GateResult,
  report: LedgerReport,
  snapshotPath: string,
): void {
  console.log("");
  if (result.ok) {
    consola.success(
      `architecture ledger gate passed (snapshot: ${snapshotPath})`,
    );
    return;
  }

  consola.error(
    `architecture ledger gate FAILED (${result.issues.length} issue(s); snapshot: ${snapshotPath})`,
  );
  for (const issue of result.issues) {
    console.log(`  [${issue.kind}] ${issue.message}`);
  }

  const denylist = report.metrics.test_real_dirs;
  if (denylist && denylist.total > 0 && denylist.samples.length > 0) {
    console.log("  denylist samples:");
    for (const sample of denylist.samples.slice(0, 12)) {
      console.log(
        `    ${sample.file}:${sample.line}: ${sample.detail ?? sample.text}`,
      );
    }
  }

  console.log("");
  console.log(
    "To accept intentional residual changes after review, regenerate the committed snapshot:",
  );
  console.log(
    `  deno task architecture-ledger --write-snapshot --snapshot ${snapshotPath}`,
  );
}
