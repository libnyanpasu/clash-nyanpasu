import * as path from "jsr:@std/path";
import { ensureDir, exists } from "jsr:@std/fs";
import AdmZip from "npm:adm-zip";
import { consola } from "./utils/logger.ts";

const FIXED_WEBVIEW_DIR = "WebView2";
type WindowsBundleVariant = "standard" | "fixed-webview";

export async function createPortableArchive(
  buildDir: string,
  tauriDir: string,
  variant: WindowsBundleVariant,
): Promise<AdmZip> {
  const configDir = path.join(buildDir, ".config");

  if (!(await exists(buildDir))) {
    throw new Error("could not found the release dir");
  }

  await ensureDir(configDir);
  await Deno.writeTextFile(path.join(configDir, "PORTABLE"), "");

  const zip = new AdmZip();
  let mainEntryPath = path.join(buildDir, "Clash Nyanpasu.exe");
  if (!(await exists(mainEntryPath))) {
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
    if (!(await exists(path.join(webviewPath, "msedgewebview2.exe")))) {
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
  if (Deno.build.os !== "windows") return;

  const cwd = Deno.cwd();
  const rustArch = Deno.env.get("RUST_ARCH") ?? "x86_64";
  const variant = Deno.args.includes("--fixed-webview")
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
    await Deno.readTextFile(path.join(cwd, "package.json")),
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
    Deno.exit(1);
  });
}
