import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { ensureDir, exists } from "jsr:@std/fs";
import * as path from "jsr:@std/path";
// @ts-types="npm:@types/adm-zip"
import AdmZip from "npm:adm-zip";
// @ts-types="npm:@types/figlet"
import figlet from "npm:figlet";
import { colorize, consola } from "./utils/logger.ts";

// === Types ===

interface BinInfo {
  name: string;
  version?: string;
  targetFile: string;
  exeFile: string;
  tmpFile: string;
  downloadURL: string;
}

type SupportedArch =
  | "windows-i386"
  | "windows-x86_64"
  | "windows-arm64"
  | "linux-aarch64"
  | "linux-amd64"
  | "linux-i386"
  | "linux-armv7"
  | "linux-armv7hf"
  | "darwin-arm64"
  | "darwin-x64";

type ArchMapping = Record<SupportedArch, string>;

interface VersionManifest {
  manifest_version: number;
  latest: {
    mihomo: string;
    mihomo_alpha: string;
    clash_rs: string;
    clash_premium: string;
    clash_rs_alpha: string;
  };
  arch_template: {
    mihomo: ArchMapping;
    mihomo_alpha: ArchMapping;
    clash_rs: ArchMapping;
    clash_premium: ArchMapping;
    clash_rs_alpha: ArchMapping;
  };
  updated_at: string;
}

interface ClashManifest {
  URL_PREFIX: string;
  BACKUP_URL_PREFIX?: string;
  BACKUP_LATEST_DATE?: string;
  VERSION?: string;
  VERSION_URL?: string;
  ARCH_MAPPING: ArchMapping;
}

interface CheckArgs {
  force?: boolean;
  arch?: string;
  "sidecar-host"?: string;
}

interface ResolveInfo {
  file: string;
  version?: string;
  size?: number;
  speed?: number;
  cached: boolean;
}

interface TaskDetail {
  version?: string;
  size?: string;
  speed?: string;
  cached?: boolean;
  file?: string;
  note?: string;
}

interface DownloadProgress {
  downloaded: number;
  total?: number;
  speed?: number;
  version?: string;
}

interface DownloadResult {
  size: number;
  speed?: number;
}

interface ResolveOptions {
  force?: boolean;
  onProgress?: (progress: DownloadProgress) => void;
}

// === Constants ===

const WORKSPACE_ROOT = path.join(import.meta.dirname!, "..");
const TAURI_APP_DIR = path.join(WORKSPACE_ROOT, "backend/tauri");
const TEMP_DIR = path.join(WORKSPACE_ROOT, "node_modules/.verge");

// === CLI Args ===

const args = parseArgs(Deno.args, {
  boolean: ["force"],
  string: ["arch", "sidecar-host"],
}) as CheckArgs;

const FORCE = args.force;
const ARCH_OVERRIDE = args.arch;
const DEBUG = Deno.env.get("LOG_LEVEL") !== undefined;

function debugLog(...args: Parameters<typeof consola.debug>) {
  if (DEBUG) consola.debug(...args);
}

const ansi = {
  bold: (text: string) => `\x1b[1m${text}\x1b[22m`,
  gray: (text: string) => `\x1b[90m${text}\x1b[39m`,
  cyan: (text: string) => `\x1b[36m${text}\x1b[39m`,
  yellow: (text: string) => `\x1b[33m${text}\x1b[39m`,
  green: (text: string) => `\x1b[32m${text}\x1b[39m`,
  red: (text: string) => `\x1b[31m${text}\x1b[39m`,
};

function formatSize(size?: number): string {
  if (size === undefined) return "";

  const units = ["B", "KB", "MB", "GB"];
  let value = size;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex++;
  }

  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function formatSpeed(bytesPerSecond?: number): string {
  if (!bytesPerSecond) return "";
  return `${formatSize(bytesPerSecond)}/s`;
}

function formatProgressSize(downloaded: number, total?: number): string {
  if (total === undefined) return formatSize(downloaded);
  return `${formatSize(downloaded)}/${formatSize(total)}`;
}

async function writeAll(file: Deno.FsFile, bytes: Uint8Array): Promise<void> {
  let written = 0;
  while (written < bytes.byteLength) {
    written += await file.write(bytes.subarray(written));
  }
}

function normalizeVersion(version?: string): string | undefined {
  if (!version) return undefined;
  if (version === "Not Found") return undefined;
  return version;
}

function formatResolveInfo(info: ResolveInfo): TaskDetail {
  return {
    version: normalizeVersion(info.version),
    size: formatSize(info.size),
    speed: formatSpeed(info.speed),
    cached: info.cached,
    file: info.file,
  };
}

// === Platform detection ===

// Deno.build.os: 'windows' | 'darwin' | 'linux' | ...
// Map to Node-style for arch table compatibility
const platform = Deno.build.os === "windows" ? "win32" : Deno.build.os;

// Deno.build.arch: 'x86_64' | 'aarch64'
// Map to Node-style for arch table compatibility
const DENO_ARCH_TO_NODE: Record<string, string> = {
  x86_64: "x64",
  aarch64: "arm64",
};
const arch = ARCH_OVERRIDE ?? DENO_ARCH_TO_NODE[Deno.build.arch] ??
  Deno.build.arch;

// === Sidecar Host ===

let SIDECAR_HOST: string | undefined = args["sidecar-host"];
if (!SIDECAR_HOST) {
  const cmd = new Deno.Command("rustc", { args: ["-vV"], stdout: "piped" });
  const { stdout } = await cmd.output();
  const text = new TextDecoder().decode(stdout);
  SIDECAR_HOST = text.match(/host: (.+)/)?.[1]?.trim();
}

if (!SIDECAR_HOST) {
  consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);
  Deno.exit(1);
}

debugLog(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`);
debugLog(colorize`platform {yellow ${platform}}`);
debugLog(colorize`arch {yellow ${arch}}`);

// === Arch Mapping ===

function mapArch(platform: string, arch: string): SupportedArch {
  const mapping: Partial<Record<string, SupportedArch>> = {
    "darwin-x64": "darwin-x64",
    "darwin-arm64": "darwin-arm64",
    "win32-x64": "windows-x86_64",
    "win32-ia32": "windows-i386",
    "win32-arm64": "windows-arm64",
    "linux-x64": "linux-amd64",
    "linux-ia32": "linux-i386",
    "linux-arm": "linux-armv7hf",
    "linux-arm64": "linux-aarch64",
    "linux-armel": "linux-armv7",
  };
  const result = mapping[`${platform}-${arch}`];
  if (!result) {
    throw new Error(`Unsupported platform/architecture: ${platform}-${arch}`);
  }
  return result;
}

// === Version Manifest ===

const versionManifest = JSON.parse(
  await Deno.readTextFile(path.join(WORKSPACE_ROOT, "manifest/version.json")),
) as VersionManifest;

const CLASH_MANIFEST: ClashManifest = {
  URL_PREFIX: "https://github.com/Dreamacro/clash/releases/download/premium/",
  BACKUP_URL_PREFIX:
    "https://github.com/zhongfly/Clash-premium-backup/releases/download/",
  BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
  VERSION: versionManifest.latest.clash_premium,
  ARCH_MAPPING: versionManifest.arch_template.clash_premium as ArchMapping,
};

const CLASH_META_MANIFEST: ClashManifest = {
  URL_PREFIX:
    `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
  VERSION: versionManifest.latest.mihomo,
  ARCH_MAPPING: versionManifest.arch_template.mihomo as ArchMapping,
};

const CLASH_META_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
  URL_PREFIX:
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha",
  ARCH_MAPPING: versionManifest.arch_template.mihomo_alpha as ArchMapping,
};

const CLASH_RS_MANIFEST: ClashManifest = {
  URL_PREFIX: "https://github.com/Watfaq/clash-rs/releases/download/",
  VERSION: versionManifest.latest.clash_rs,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs as ArchMapping,
};

const CLASH_RS_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    "https://github.com/Watfaq/clash-rs/releases/download/latest/version.txt",
  URL_PREFIX: "https://github.com/Watfaq/clash-rs/releases/download/latest",
  ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha as ArchMapping,
};

// === Download ===

async function downloadFile(
  url: string,
  filePath: string,
  onProgress?: (progress: DownloadProgress) => void,
): Promise<DownloadResult> {
  debugLog(colorize`downloading {gray "${url.split("/").at(-1)}"}`);

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

// === Extract Helpers ===

async function extractZip(
  zipPath: string,
  destDir: string,
  name: string,
): Promise<string> {
  const zip = new AdmZip(zipPath);
  const baseName = name
    .split("-")
    .filter((o: string) => o !== "alpha")
    .join("-");
  let entryName: string | undefined;

  for (const entry of zip.getEntries()) {
    debugLog(colorize`"{green ${name}}" entry name ${entry.entryName}`);
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

async function extractTarGz(
  tarPath: string,
  destDir: string,
  name: string,
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

async function gunzipFile(
  inputPath: string,
  outputPath: string,
): Promise<void> {
  const input = await Deno.open(inputPath, { read: true });
  const output = await Deno.open(outputPath, { write: true, create: true });
  await input.readable
    .pipeThrough(new DecompressionStream("gzip"))
    .pipeTo(output.writable);
}

// === Resource Resolution ===

async function resolveResource(
  binInfo: { file: string; downloadURL: string; version?: string },
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const { file, downloadURL, version } = binInfo;
  const resDir = path.join(TAURI_APP_DIR, "resources");
  const targetPath = path.join(resDir, file);

  if (!options?.force && (await exists(targetPath))) {
    return {
      file,
      version,
      size: (await Deno.stat(targetPath)).size,
      cached: true,
    };
  }

  await ensureDir(resDir);
  const { size, speed } = await downloadFile(
    downloadURL,
    targetPath,
    (progress) => options?.onProgress?.({ ...progress, version }),
  );

  debugLog(colorize`resolve {green ${file}} finished`);
  return { file, version, size, speed, cached: false };
}

async function resolveSidecar(
  binInfo: BinInfo | Promise<BinInfo>,
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const { name, version, targetFile, tmpFile, exeFile, downloadURL } =
    await binInfo;

  const sidecarDir = path.join(TAURI_APP_DIR, "sidecar");
  const sidecarPath = path.join(sidecarDir, targetFile);

  await ensureDir(sidecarDir);

  if (!options?.force && (await exists(sidecarPath))) {
    return {
      file: targetFile,
      version,
      size: (await Deno.stat(sidecarPath)).size,
      cached: true,
    };
  }

  const tempDir = path.join(TEMP_DIR, name);
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
      );
      size = result.size;
      speed = result.speed;
    } else {
      size = (await Deno.stat(tempFile)).size;
    }

    if (tmpFile.endsWith(".zip")) {
      const extractedExe = await extractZip(tempFile, tempDir, name);
      await Deno.rename(extractedExe, tempExe);
      await Deno.rename(tempExe, sidecarPath);
    } else if (tmpFile.endsWith(".tar.gz")) {
      await extractTarGz(tempFile, tempDir, name);
      await Deno.rename(tempExe, sidecarPath);
    } else if (tmpFile.endsWith(".gz")) {
      await gunzipFile(tempFile, sidecarPath);
      await Deno.chmod(sidecarPath, 0o755);
    } else {
      await Deno.rename(tempFile, sidecarPath);
      if (platform !== "win32") {
        await Deno.chmod(sidecarPath, 0o755);
      }
    }

    debugLog(colorize`resolve {green ${name}} finished`);
    return { file: targetFile, version, size, speed, cached: false };
  } catch (err) {
    try {
      await Deno.remove(sidecarPath);
    } catch {
      // ignore
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

// === Binary Info Functions ===

function getClashBackupInfo(): BinInfo {
  const { ARCH_MAPPING, BACKUP_URL_PREFIX, BACKUP_LATEST_DATE } =
    CLASH_MANIFEST;
  const archLabel = mapArch(platform, arch);
  const name = ARCH_MAPPING[archLabel].replace("{}", BACKUP_LATEST_DATE!);
  const isWin = platform === "win32";
  return {
    name: "clash",
    version: BACKUP_LATEST_DATE,
    targetFile: `clash-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: `${name}${isWin ? ".exe" : ""}`,
    tmpFile: name,
    downloadURL: `${BACKUP_URL_PREFIX}${BACKUP_LATEST_DATE}/${name}`,
  };
}

function getClashMetaInfo(): BinInfo {
  const { ARCH_MAPPING, URL_PREFIX, VERSION } = CLASH_META_MANIFEST;
  const archLabel = mapArch(platform, arch);
  const name = ARCH_MAPPING[archLabel].replace("{}", VERSION!);
  const isWin = platform === "win32";
  return {
    name: "mihomo",
    version: VERSION,
    targetFile: `mihomo-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: `${name}${isWin ? ".exe" : ""}`,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  };
}

async function getClashMetaAlphaInfo(): Promise<BinInfo> {
  const { ARCH_MAPPING, URL_PREFIX, VERSION_URL } = CLASH_META_ALPHA_MANIFEST;
  const resp = await fetch(VERSION_URL!);
  const version = normalizeVersion((await resp.text()).trim()) ??
    versionManifest.latest.mihomo_alpha;
  debugLog(`mihomo-alpha version: ${version}`);
  const archLabel = mapArch(platform, arch);
  const name = ARCH_MAPPING[archLabel].replace("{}", version);
  const isWin = platform === "win32";
  return {
    name: "mihomo-alpha",
    version,
    targetFile: `mihomo-alpha-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: `${name}${isWin ? ".exe" : ""}`,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  };
}

function getClashRustInfo(): BinInfo {
  const { ARCH_MAPPING, URL_PREFIX, VERSION } = CLASH_RS_MANIFEST;
  const archLabel = mapArch(platform, arch);
  const name = ARCH_MAPPING[archLabel].replace("{}", VERSION!);
  const isWin = platform === "win32";
  return {
    name: "clash-rs",
    version: VERSION,
    targetFile: `clash-rs-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: name,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}${VERSION}/${name}`,
  };
}

async function getClashRustAlphaInfo(): Promise<BinInfo> {
  const { ARCH_MAPPING, VERSION_URL, URL_PREFIX } = CLASH_RS_ALPHA_MANIFEST;

  const resp = await fetch(VERSION_URL!);
  const version = normalizeVersion((await resp.text()).trim()) ??
    versionManifest.latest.clash_rs_alpha;
  debugLog(`clash-rs-alpha version: ${version}`);
  const archLabel = mapArch(platform, arch);
  const name = ARCH_MAPPING[archLabel].replace("{}", version);
  const isWin = platform === "win32";
  return {
    name: "clash-rs-alpha",
    version,
    targetFile: `clash-rs-alpha-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: name,
    tmpFile: name,
    downloadURL: `${URL_PREFIX}/${name}`,
  };
}

async function getNyanpasuServiceInfo(): Promise<BinInfo> {
  const SERVICE_REPO = "libnyanpasu/nyanpasu-service";
  const isWin = SIDECAR_HOST!.includes("windows");
  const urlExt = isWin ? "zip" : "tar.gz";

  const response = await fetch(
    `https://github.com/${SERVICE_REPO}/releases/latest`,
    { method: "GET", redirect: "manual" },
  );
  const location = response.headers.get("location");
  if (!location) throw new Error("Cannot find location from response header");
  const version = location.split("/").pop();
  if (!version) throw new Error("Cannot find tag from location");
  debugLog(`nyanpasu-service version: ${version}`);

  const name = "nyanpasu-service";
  return {
    name,
    version,
    targetFile: `${name}-${SIDECAR_HOST}${isWin ? ".exe" : ""}`,
    exeFile: `${name}${isWin ? ".exe" : ""}`,
    tmpFile: `${name}-${SIDECAR_HOST}.${urlExt}`,
    downloadURL:
      `https://github.com/${SERVICE_REPO}/releases/download/${version}/${name}-${SIDECAR_HOST}.${urlExt}`,
  };
}

async function resolveWintun(
  onProgress?: (progress: DownloadProgress) => void,
): Promise<ResolveInfo> {
  if (platform !== "win32") {
    return {
      file: "wintun.dll",
      cached: true,
    };
  }

  const wintunArchMap: Record<string, string> = {
    x64: "amd64",
    ia32: "x86",
    arm: "arm",
    arm64: "arm64",
  };
  const wintunArch = wintunArchMap[arch];
  if (!wintunArch) throw new Error(`unsupported arch ${arch}`);

  const url = "https://www.wintun.net/builds/wintun-0.14.1.zip";
  const expectedHash =
    "07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51";
  const tempDir = path.join(TEMP_DIR, "wintun");
  const tempZip = path.join(tempDir, "wintun.zip");
  const targetPath = path.join(TAURI_APP_DIR, "resources", "wintun.dll");

  if (!FORCE && (await exists(targetPath))) {
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
    const result = await downloadFile(url, tempZip, onProgress);
    size = result.size;
    speed = result.speed;
  } else {
    size = (await Deno.stat(tempZip)).size;
  }

  // verify SHA-256
  const fileData = await Deno.readFile(tempZip);
  const hashBuffer = await crypto.subtle.digest("SHA-256", fileData);
  const hashHex = Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  if (hashHex !== expectedHash) {
    throw new Error(`wintun hash not match ${hashHex}`);
  }

  // extract
  const zip = new AdmZip(tempZip);
  zip.extractAllTo(tempDir, true);

  // recursively find wintun.dll for the target arch
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

  const dlls = findDlls(tempDir);
  const dll = dlls[0];
  if (!dll) throw new Error(`wintun not found for arch ${wintunArch}`);

  await ensureDir(path.dirname(targetPath));
  await Deno.copyFile(dll, targetPath);
  await Deno.remove(tempDir, { recursive: true });

  debugLog(colorize`resolve {green wintun.dll} finished`);
  return { file: "wintun.dll", size, speed, cached: false };
}

// === Progress Renderer ===

type TaskStatus = "Waiting" | "Pulling" | "Retrying" | "Done" | "Failed";

class ProgressRenderer {
  readonly enabled = Deno.stdout.isTerminal() && !Deno.env.get("CI") && !DEBUG;
  private readonly renderInterval = 120;

  private readonly encoder = new TextEncoder();
  private readonly width: number;
  private readonly status = new Map<
    string,
    { status: TaskStatus; detail: TaskDetail }
  >();
  private lineCount = 0;
  private rendered = false;
  private lastRenderAt = 0;
  private renderTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(names: string[]) {
    this.width = Math.max(...names.map((name) => name.length), 0);
    for (const name of names) {
      this.status.set(name, { status: "Waiting", detail: {} });
    }
  }

  start() {
    if (!this.enabled) {
      consola.start("start check and download resources...");
      return;
    }

    this.write("\x1b[?25l");
    this.render();
  }

  update(name: string, status: TaskStatus, detail: TaskDetail = {}) {
    const previous = this.status.get(name);
    this.status.set(name, { status, detail });

    if (!this.enabled) {
      if (status === "Pulling" && previous?.status === "Pulling") return;
      const detailText = this.formatDetailText(detail);
      if (status === "Pulling") consola.info(`${name} Pulling ${detailText}`);
      if (status === "Retrying") consola.warn(`${name} Retrying ${detailText}`);
      if (status === "Done") consola.success(`${name} Done ${detailText}`);
      return;
    }

    this.queueRender(status !== "Pulling");
  }

  finish() {
    if (!this.enabled) return;

    if (this.renderTimer) {
      clearTimeout(this.renderTimer);
      this.renderTimer = undefined;
    }
    this.render();
    this.write("\x1b[?25h");
  }

  private queueRender(immediate = false) {
    if (immediate) {
      if (this.renderTimer) {
        clearTimeout(this.renderTimer);
        this.renderTimer = undefined;
      }
      this.render();
      return;
    }

    const elapsed = performance.now() - this.lastRenderAt;
    if (elapsed >= this.renderInterval) {
      this.render();
      return;
    }

    if (this.renderTimer) return;
    this.renderTimer = setTimeout(() => {
      this.renderTimer = undefined;
      this.render();
    }, this.renderInterval - elapsed);
  }

  private render() {
    const lines = [...this.status.entries()].map(([name, { status, detail }]) =>
      `${this.formatStatus(status)} ${ansi.bold(name.padEnd(this.width))}  ${
        this.formatDetail(
          detail,
        )
      }`.trimEnd()
    );

    const output = `${this.rendered ? `\x1b[${this.lineCount}A\x1b[J` : ""}${
      lines.join(
        "\n",
      )
    }\n`;

    this.write(output);
    this.lineCount = lines.length;
    this.rendered = true;
    this.lastRenderAt = performance.now();
  }

  private formatStatus(status: TaskStatus) {
    switch (status) {
      case "Waiting":
        return ansi.gray("Waiting".padEnd(8));
      case "Pulling":
        return ansi.cyan("Pulling".padEnd(8));
      case "Retrying":
        return ansi.yellow("Retrying".padEnd(8));
      case "Done":
        return ansi.green("Done".padEnd(8));
      case "Failed":
        return ansi.red("Failed".padEnd(8));
    }
  }

  private formatDetail(detail: TaskDetail): string {
    const version = detail.version
      ? `${ansi.gray("version")} ${ansi.yellow(detail.version.padEnd(24))}`
      : " ".repeat(32);
    const size = detail.size
      ? `${ansi.gray("size")} ${ansi.cyan(detail.size.padStart(15))}`
      : " ".repeat(20);
    const speed = detail.speed
      ? `${ansi.gray("speed")} ${ansi.cyan(detail.speed.padStart(12))}`
      : " ".repeat(18);
    const cached = detail.cached ? ansi.gray("cached") : " ".repeat(6);
    const file = detail.file ? ansi.gray(detail.file) : "";
    const note = detail.note ? ansi.gray(detail.note) : "";

    return `${version}  ${size}  ${speed}  ${cached}  ${file || note}`
      .trimEnd();
  }

  private formatDetailText(detail: TaskDetail): string {
    return [
      detail.version ? `version=${detail.version}` : "",
      detail.size ? `size=${detail.size}` : "",
      detail.speed ? `speed=${detail.speed}` : "",
      detail.cached ? "cached" : "",
      detail.file,
      detail.note,
    ]
      .filter(Boolean)
      .join(" ");
  }

  private write(text: string) {
    Deno.stdout.writeSync(this.encoder.encode(text));
  }
}

// === Task Runner ===

const tasks: Array<{
  name: string;
  version?: string;
  func: (
    onProgress?: (progress: DownloadProgress) => void,
  ) => Promise<ResolveInfo>;
  retry: number;
  winOnly?: boolean;
}> = [
  {
    name: "clash",
    version: versionManifest.latest.clash_premium,
    func: (onProgress) =>
      resolveSidecar(getClashBackupInfo(), {
        force: FORCE,
        onProgress,
      }),
    retry: 5,
  },
  {
    name: "mihomo",
    version: versionManifest.latest.mihomo,
    func: (onProgress) =>
      resolveSidecar(getClashMetaInfo(), { force: FORCE, onProgress }),
    retry: 5,
  },
  {
    name: "mihomo-alpha",
    func: (onProgress) =>
      resolveSidecar(getClashMetaAlphaInfo(), { force: FORCE, onProgress }),
    retry: 5,
  },
  {
    name: "clash-rs",
    version: versionManifest.latest.clash_rs,
    func: (onProgress) =>
      resolveSidecar(getClashRustInfo(), { force: FORCE, onProgress }),
    retry: 5,
  },
  {
    name: "clash-rs-alpha",
    func: (onProgress) =>
      resolveSidecar(getClashRustAlphaInfo(), { force: FORCE, onProgress }),
    retry: 5,
  },
  {
    name: "wintun",
    func: (onProgress) => resolveWintun(onProgress),
    retry: 5,
    winOnly: true,
  },
  {
    name: "nyanpasu-service",
    func: (onProgress) =>
      resolveSidecar(getNyanpasuServiceInfo(), { force: FORCE, onProgress }),
    retry: 5,
  },
  {
    name: "mmdb",
    func: (onProgress) =>
      resolveResource(
        {
          file: "Country.mmdb",
          downloadURL:
            "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb",
        },
        { force: FORCE, onProgress },
      ),
    retry: 5,
  },
  {
    name: "geoip",
    func: (onProgress) =>
      resolveResource(
        {
          file: "geoip.dat",
          downloadURL:
            "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
        },
        { force: FORCE, onProgress },
      ),
    retry: 5,
  },
  {
    name: "geosite",
    func: (onProgress) =>
      resolveResource(
        {
          file: "geosite.dat",
          downloadURL:
            "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
        },
        { force: FORCE, onProgress },
      ),
    retry: 5,
  },
  {
    name: "enableLoopback",
    func: (onProgress) =>
      resolveResource(
        {
          file: "enableLoopback.exe",
          downloadURL:
            "https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe",
        },
        { force: FORCE, onProgress },
      ),
    retry: 5,
    winOnly: true,
  },
];

async function runTask(
  queue: typeof tasks,
  progress: ProgressRenderer,
): Promise<void> {
  const task = queue.shift();
  if (!task) return;

  for (let i = 0; i < task.retry; i++) {
    try {
      progress.update(
        task.name,
        "Pulling",
        task.version ? { version: task.version } : {},
      );
      const info = await task.func((download) => {
        progress.update(task.name, "Pulling", {
          version: normalizeVersion(download.version) ?? task.version,
          size: formatProgressSize(download.downloaded, download.total),
          speed: formatSpeed(download.speed),
        });
      });
      progress.update(task.name, "Done", formatResolveInfo(info));
      break;
    } catch (err) {
      if (i === task.retry - 1) {
        progress.update(task.name, "Failed");
        progress.finish();
        consola.fatal(`task::${task.name} failed`, err);
        Deno.exit(1);
      }
      progress.update(task.name, "Retrying", {
        note: `${i + 1}/${task.retry}`,
        version: task.version,
      });
    }
  }

  return runTask(queue, progress);
}

// === Main ===

const activeTasks = tasks.filter(
  (task) => !task.winOnly || platform === "win32",
);
const progress = new ProgressRenderer(activeTasks.map((task) => task.name));
progress.start();

const concurrency = Math.ceil(navigator.hardwareConcurrency / 2) || 2;
const queue = [...activeTasks];
const jobs = Array.from(
  { length: concurrency },
  () => runTask(queue, progress),
);

await Promise.all(jobs);
progress.finish();

console.log(figlet.textSync("Clash Nyanpasu", { whitespaceBreak: true }));
consola.success("all resources download finished\n");
consola.log("  next command:\n");
consola.log("    pnpm dev - development");
consola.log("    pnpm dev:diff - deadlock development (recommend)");
