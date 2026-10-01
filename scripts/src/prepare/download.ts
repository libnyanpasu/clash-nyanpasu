import * as path from "jsr:@std/path";
// @ts-types="npm:@types/adm-zip"
import AdmZip from "npm:adm-zip";
import type { DownloadProgress, DownloadResult } from "./types.ts";

export type DebugLog = (message: string) => void;

async function writeAll(file: Deno.FsFile, bytes: Uint8Array): Promise<void> {
  let written = 0;
  while (written < bytes.byteLength) {
    written += await file.write(bytes.subarray(written));
  }
}

export async function downloadFile(
  url: string,
  filePath: string,
  onProgress?: (progress: DownloadProgress) => void,
  debugLog: DebugLog = () => {},
): Promise<DownloadResult> {
  debugLog(`downloading "${url.split("/").at(-1)}"`);

  const response = await fetch(url, {
    method: "GET",
    headers: {
      "Content-Type": "application/octet-stream",
      "User-Agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0",
    },
  });

  if (!response.ok) {
    throw new Error(
      `download failed: ${response.statusText} (${response.status})`,
    );
  }

  const totalHeader = response.headers.get("content-length");
  const total = totalHeader ? Number.parseInt(totalHeader, 10) : undefined;
  const startedAt = performance.now();
  let downloaded = 0;

  const file = await Deno.open(filePath, {
    create: true,
    truncate: true,
    write: true,
  });

  try {
    if (!response.body) throw new Error("download failed: empty response body");

    const reader = response.body.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      await writeAll(file, value);
      downloaded += value.byteLength;
      const elapsedSeconds = Math.max(
        (performance.now() - startedAt) / 1000,
        0.001,
      );
      onProgress?.({
        downloaded,
        total,
        speed: downloaded / elapsedSeconds,
      });
    }
  } finally {
    file.close();
  }

  const elapsedSeconds = Math.max(
    (performance.now() - startedAt) / 1000,
    0.001,
  );
  return { size: downloaded, speed: downloaded / elapsedSeconds };
}

export async function extractZip(
  zipPath: string,
  destDir: string,
  name: string,
  debugLog: DebugLog = () => {},
): Promise<string> {
  const zip = new AdmZip(zipPath);
  const baseName = name
    .split("-")
    .filter((o: string) => o !== "alpha")
    .join("-");
  let entryName: string | undefined;

  for (const entry of zip.getEntries()) {
    debugLog(`"${name}" entry name ${entry.entryName}`);
    if (
      (entry.entryName.includes(name) && entry.entryName.endsWith(".exe")) ||
      (entry.entryName.includes(baseName) && entry.entryName.endsWith(".exe"))
    ) {
      entryName = entry.entryName;
    }
  }

  zip.extractAllTo(destDir, true);
  if (!entryName) throw new Error("cannot find exe file in zip");
  return path.join(destDir, entryName);
}

export async function extractTarGz(
  tarPath: string,
  destDir: string,
): Promise<void> {
  const cmd = new Deno.Command("tar", {
    args: ["-xzf", tarPath, "-C", destDir],
    stdout: "piped",
    stderr: "piped",
  });
  const { code, stderr } = await cmd.output();
  if (code !== 0) {
    throw new Error(
      `tar extraction failed: ${new TextDecoder().decode(stderr)}`,
    );
  }
}

export async function gunzipFile(
  inputPath: string,
  outputPath: string,
): Promise<void> {
  const input = await Deno.open(inputPath, { read: true });
  const output = await Deno.open(outputPath, { write: true, create: true });
  await input.readable
    .pipeThrough(new DecompressionStream("gzip"))
    .pipeTo(output.writable);
}
