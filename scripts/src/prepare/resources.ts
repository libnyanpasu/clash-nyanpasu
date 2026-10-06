import { ensureDir, exists } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
// @ts-types="npm:@types/adm-zip"
import AdmZip from "npm:adm-zip";
import type {
  BinInfo,
  DownloadProgress,
  ResolveInfo,
  ResolveOptions,
} from "./types.ts";
import {
  downloadFile,
  extractTarGz,
  extractZip,
  gunzipFile,
} from "./download.ts";
import type { DebugLog } from "./download.ts";
import { normalizeArch, normalizePlatform } from "./platform.ts";

export interface ResourceResolverOptions {
  resourcesDir: string;
  sidecarDir: string;
  tempRoot: string;
  platform: string;
  arch: string;
  debugLog?: DebugLog;
  runMeowVersion?: (executablePath: string) => Promise<string>;
}

export async function resolveResource(
  context: ResourceResolverOptions,
  binInfo: { file: string; downloadURL: string; version?: string },
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const { file, downloadURL, version } = binInfo;
  const targetPath = path.join(context.resourcesDir, file);
  const debugLog = context.debugLog ?? (() => {});

  if (!options?.force && (await exists(targetPath))) {
    return {
      file,
      version,
      size: (await Deno.stat(targetPath)).size,
      cached: true,
    };
  }

  await ensureDir(context.resourcesDir);
  const { size, speed } = await downloadFile(
    downloadURL,
    targetPath,
    (progress) => options?.onProgress?.({ ...progress, version }),
    debugLog,
  );
  debugLog(`resolve ${file} finished`);
  return { file, version, size, speed, cached: false };
}

async function readVersionStamp(stampPath: string): Promise<string | null> {
  try {
    return (await Deno.readTextFile(stampPath)).trim();
  } catch {
    return null;
  }
}

function hasExecutableFormat(bytes: Uint8Array, platform: string): boolean {
  if (platform === "win32") return bytes[0] === 0x4d && bytes[1] === 0x5a;
  if (platform === "linux") {
    return bytes[0] === 0x7f && bytes[1] === 0x45 && bytes[2] === 0x4c &&
      bytes[3] === 0x46;
  }
  if (platform === "darwin") {
    const magic = Array.from(bytes.slice(0, 4)).map((byte) =>
      byte.toString(16).padStart(2, "0")
    ).join("");
    return ["cffaedfe", "cefaedfe", "feedfacf", "feedface", "cafebabe"]
      .includes(magic);
  }
  return false;
}

export function assertMeowVersion(
  output: string,
  expectedVersion: string,
): void {
  const actualVersion = output.trim().split(/\s+/).at(-1);
  if (!actualVersion) throw new Error("meow -v returned no version");

  if (expectedVersion.startsWith("alpha-")) {
    const expectedSha = expectedVersion.slice("alpha-".length);
    if (!actualVersion.endsWith(`-alpha+${expectedSha}`)) {
      throw new Error(`unexpected Meow alpha version: ${actualVersion}`);
    }
  } else if (
    actualVersion.replace(/^v/, "") !== expectedVersion.replace(/^v/, "")
  ) {
    throw new Error(`unexpected Meow version: ${actualVersion}`);
  }
}

async function verifyMeowExecutable(
  executablePath: string,
  platform: string,
  arch: string,
  version: string,
  runVersion = runMeowVersion,
): Promise<void> {
  const bytes = await Deno.readFile(executablePath);
  if (!hasExecutableFormat(bytes, platform)) {
    throw new Error(`invalid ${platform} executable format for Meow`);
  }

  if (
    platform !== normalizePlatform(Deno.build.os) ||
    arch !== normalizeArch(Deno.build.arch)
  ) {
    return;
  }

  const output = (await runVersion(executablePath)).trim();
  if (!output) throw new Error("meow -v returned no version");
  assertMeowVersion(output, version);
}

async function runMeowVersion(executablePath: string): Promise<string> {
  const result = await new Deno.Command(executablePath, {
    args: ["-v"],
    stdout: "piped",
    stderr: "piped",
  }).output();
  const output = new TextDecoder().decode(result.stdout).trim();
  if (result.code !== 0 || !output) {
    const stderr = new TextDecoder().decode(result.stderr).trim();
    throw new Error(`meow -v failed (${result.code}): ${stderr}`);
  }
  return output;
}

export async function resolveSidecar(
  context: ResourceResolverOptions,
  binInfo: BinInfo | Promise<BinInfo>,
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const { name, version, targetFile, tmpFile, exeFile, downloadURL } =
    await binInfo;
  const sidecarPath = path.join(context.sidecarDir, targetFile);
  const versionStampPath = `${sidecarPath}.version`;
  const debugLog = context.debugLog ?? (() => {});

  await ensureDir(context.sidecarDir);

  // These cached targets are only trusted alongside a matching version stamp
  // written after successful materialization.
  const needsVersionStamp = ["nyanpasu-service", "meow", "meow-alpha"]
    .includes(name);
  const cachedTargetIsValid = needsVersionStamp
    ? (await exists(sidecarPath)) &&
      (await readVersionStamp(versionStampPath)) === version
    : await exists(sidecarPath);

  if (!options?.force && cachedTargetIsValid) {
    return {
      file: targetFile,
      version,
      size: (await Deno.stat(sidecarPath)).size,
      cached: true,
    };
  }

  const tempDir = path.join(context.tempRoot, name);
  const tempFile = path.join(tempDir, tmpFile);
  const tempExe = path.join(tempDir, exeFile);
  await ensureDir(tempDir);

  try {
    let size: number;
    let speed: number | undefined;
    if (!(await exists(tempFile))) {
      const result = await downloadFile(
        downloadURL,
        tempFile,
        (progress) => options?.onProgress?.({ ...progress, version }),
        debugLog,
      );
      size = result.size;
      speed = result.speed;
    } else {
      size = (await Deno.stat(tempFile)).size;
    }

    const isMeow = name === "meow" || name === "meow-alpha";
    if (tmpFile.endsWith(".zip")) {
      const extractedExe = await extractZip(tempFile, tempDir, name, debugLog);
      await Deno.rename(extractedExe, tempExe);
      if (!isMeow) await Deno.rename(tempExe, sidecarPath);
    } else if (tmpFile.endsWith(".tar.gz")) {
      await extractTarGz(tempFile, tempDir);
      if (!isMeow) await Deno.rename(tempExe, sidecarPath);
    } else if (tmpFile.endsWith(".gz")) {
      await gunzipFile(tempFile, sidecarPath);
      await Deno.chmod(sidecarPath, 0o755);
    } else {
      await Deno.rename(tempFile, sidecarPath);
      if (context.platform !== "win32") await Deno.chmod(sidecarPath, 0o755);
    }

    if (isMeow) {
      if (context.platform !== "win32") {
        await Deno.chmod(tempExe, 0o755);
      }
      if (!version) throw new Error(`missing version for ${name}`);
      await verifyMeowExecutable(
        tempExe,
        context.platform,
        context.arch,
        version,
        context.runMeowVersion,
      );
      await Deno.rename(tempExe, sidecarPath);
    }

    if (needsVersionStamp && version) {
      await Deno.writeTextFile(versionStampPath, version);
    }

    debugLog(`resolve ${name} finished`);
    return { file: targetFile, version, size, speed, cached: false };
  } catch (err) {
    if (!needsVersionStamp) {
      try {
        await Deno.remove(sidecarPath);
      } catch {
        // ignore
      }
    }
    throw err;
  } finally {
    try {
      await Deno.remove(tempDir, { recursive: true });
    } catch {
      // ignore
    }
  }
}

export async function resolveWintun(
  context: ResourceResolverOptions,
  force: boolean,
  onProgress?: (progress: DownloadProgress) => void,
): Promise<ResolveInfo> {
  if (context.platform !== "win32") {
    return { file: "wintun.dll", cached: true };
  }

  const wintunArchMap: Record<string, string> = {
    x64: "amd64",
    ia32: "x86",
    arm: "arm",
    arm64: "arm64",
  };
  const wintunArch = wintunArchMap[context.arch];
  if (!wintunArch) throw new Error(`unsupported arch ${context.arch}`);

  const url = "https://www.wintun.net/builds/wintun-0.14.1.zip";
  const expectedHash =
    "07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51";
  const tempDir = path.join(context.tempRoot, "wintun");
  const tempZip = path.join(tempDir, "wintun.zip");
  const targetPath = path.join(context.resourcesDir, "wintun.dll");
  const debugLog = context.debugLog ?? (() => {});

  if (!force && (await exists(targetPath))) {
    return {
      file: "wintun.dll",
      size: (await Deno.stat(targetPath)).size,
      cached: true,
    };
  }

  await ensureDir(tempDir);
  let size: number;
  let speed: number | undefined;
  if (!(await exists(tempZip))) {
    const result = await downloadFile(url, tempZip, onProgress, debugLog);
    size = result.size;
    speed = result.speed;
  } else {
    size = (await Deno.stat(tempZip)).size;
  }

  const fileData = await Deno.readFile(tempZip);
  const hashBuffer = await crypto.subtle.digest("SHA-256", fileData);
  const hashHex = Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  if (hashHex !== expectedHash) {
    throw new Error(`wintun hash not match ${hashHex}`);
  }

  const zip = new AdmZip(tempZip);
  zip.extractAllTo(tempDir, true);

  function findDlls(dir: string): string[] {
    const results: string[] = [];
    for (const entry of Deno.readDirSync(dir)) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory) {
        results.push(...findDlls(fullPath));
      } else if (entry.name === "wintun.dll" && fullPath.includes(wintunArch)) {
        results.push(fullPath);
      }
    }
    return results;
  }

  const dll = findDlls(tempDir)[0];
  if (!dll) throw new Error(`wintun not found for arch ${wintunArch}`);

  await ensureDir(path.dirname(targetPath));
  await Deno.copyFile(dll, targetPath);
  await Deno.remove(tempDir, { recursive: true });
  debugLog("resolve wintun.dll finished");
  return { file: "wintun.dll", size, speed, cached: false };
}
