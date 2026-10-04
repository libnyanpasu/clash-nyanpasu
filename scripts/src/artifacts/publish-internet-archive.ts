import * as path from "jsr:@std/path";
import { runArchivePublish } from "./internet-archive-upload.ts";

async function main(): Promise<void> {
  const [manifest, server, report] = Deno.args;
  if (!manifest || !server || !report) {
    throw new Error(
      "Usage: archive:publish <manifest.json> <server-url> <report.json>",
    );
  }
  const reportPath = path.resolve(report);
  await Deno.mkdir(path.dirname(reportPath), { recursive: true });
  await Deno.remove(reportPath).catch((error) => {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  });
  Deno.exitCode = await runArchivePublish([
    "--manifest",
    path.resolve(manifest),
    "--server",
    server,
    "--report",
    reportPath,
  ]);
}

if (import.meta.main) await main();
