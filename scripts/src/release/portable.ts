import * as path from "node:path";
import { pathExists } from "../shared/path-exists.ts";
import AdmZip from "npm:adm-zip";
import { consola } from "../shared/logger.ts";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const FIXED_WEBVIEW_DIR = "WebView2";
type WindowsBundleVariant = "standard" | "fixed-webview";

export async function createPortableArchive(
  buildDir: string,
  tauriDir: string,
  variant: WindowsBundleVariant,
): Promise<AdmZip> {
  const configDir = path.join(buildDir, ".config");

  if (!(await pathExists(buildDir))) {
    throw new Error("could not found the release dir");
  }

  await mkdir(configDir, { recursive: true });
  await writeFile(path.join(configDir, "PORTABLE"), "");

  const zip = new AdmZip();
  let mainEntryPath = path.join(buildDir, "Clash Nyanpasu.exe");
  if (!(await pathExists(mainEntryPath))) {
    mainEntryPath = path.join(buildDir, "clash-nyanpasu.exe");
  }
  zip.addLocalFile(mainEntryPath);
  zip.addLocalFile(path.join(buildDir, "clash.exe"));
  zip.addLocalFile(path.join(buildDir, "mihomo.exe"));
  zip.addLocalFile(path.join(buildDir, "mihomo-alpha.exe"));
  zip.addLocalFile(path.join(buildDir, "nyanpasu-service.exe"));
  zip.addLocalFile(path.join(buildDir, "clash-rs.exe"));
  zip.addLocalFile(path.join(buildDir, "clash-rs-alpha.exe"));
  zip.addLocalFolder(path.join(buildDir, "resources"), "resources");

  if (variant === "fixed-webview") {
    const webviewPath = path.join(tauriDir, FIXED_WEBVIEW_DIR);
    if (!(await pathExists(path.join(webviewPath, "msedgewebview2.exe")))) {
      throw new Error("WebView2 runtime not found");
    }
    zip.addLocalFolder(
      webviewPath,
      FIXED_WEBVIEW_DIR,
    );
  }

  zip.addLocalFolder(configDir, ".config");
  return zip;
}

async function resolvePortable() {
  if (process.platform !== "win32") return;

  const cwd = process.cwd();
  const rustArch = process.env["RUST_ARCH"] ?? "x86_64";
  const variant = process.argv.slice(2).includes("--fixed-webview")
    ? "fixed-webview"
    : "standard";
  const buildDir = rustArch === "x86_64"
    ? "backend/target/release"
    : `backend/target/${rustArch}-pc-windows-msvc/release`;
  const zip = await createPortableArchive(
    path.join(cwd, buildDir),
    path.join(cwd, "backend/tauri"),
    variant,
  );

  const packageJson = JSON.parse(
    await readFile(path.join(cwd, "package.json"), "utf8"),
  );
  const version = packageJson.version;

  const zipFile = `Clash.Nyanpasu_${version}_${rustArch}${
    variant === "fixed-webview" ? "_fixed-webview" : ""
  }_portable.zip`;
  zip.writeZip(zipFile);

  consola.success("create portable zip successfully");
}

if (import.meta.main) {
  resolvePortable().catch((err) => {
    consola.error(err);
    process.exit(1);
  });
}
