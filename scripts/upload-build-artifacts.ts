import * as path from "jsr:@std/path";
import { globby } from "npm:globby";
import { uploadAllFiles } from "./utils/file-server.ts";
import { consola } from "./utils/logger.ts";

function requireEnv(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    consola.fatal(`${name} is required`);
    Deno.exit(1);
  }
  return value;
}

const FILE_SERVER_TOKEN = requireEnv("FILE_SERVER_TOKEN");
const FOLDER_PATH = requireEnv("FOLDER_PATH");

const patterns = Deno.args;
if (patterns.length === 0) {
  consola.fatal("No file patterns provided as arguments");
  Deno.exit(1);
}

const WORKSPACE_ROOT = path.join(import.meta.dirname!, "..");

consola.info(`Searching for files matching: ${patterns.join(", ")}`);
const files = await globby(patterns, { cwd: WORKSPACE_ROOT, absolute: true });

consola.info(`Found ${files.length} files:`);
for (const f of files) {
  consola.info(`  ${path.basename(f)}`);
}

const results = files.length > 0
  ? await uploadAllFiles(files, FILE_SERVER_TOKEN, FOLDER_PATH)
  : [];

const outputPath = path.join(WORKSPACE_ROOT, "upload-results.json");
await Deno.writeTextFile(outputPath, JSON.stringify(results, null, 2));
consola.success(
  `Upload complete. ${results.length} files uploaded. Results written to ${outputPath}`,
);
