import * as path from "jsr:@std/path";
import { ensureDir, exists } from "jsr:@std/fs";
import AdmZip from "npm:adm-zip";
import { consola } from "./utils/logger.ts";

const RUST_ARCH = Deno.env.get("RUST_ARCH") ?? "x86_64";
const fixedWebview = Deno.args.includes("--fixed-webview");

async function resolvePortable() {
  if (Deno.build.os !== "windows") return;

  const cwd = Deno.cwd();
  const TAURI_APP_DIR = path.join(cwd, "backend/tauri");

  const buildDir = RUST_ARCH === "x86_64"
    ? "backend/target/release"
    : `backend/target/${RUST_ARCH}-pc-windows-msvc/release`;

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

  if (fixedWebview) {
    let webviewPath: string | undefined;
    for await (const entry of Deno.readDir(TAURI_APP_DIR)) {
      if (entry.name.includes("WebView2")) {
        webviewPath = entry.name;
        break;
      }
    }
    if (!webviewPath) {
      throw new Error("WebView2 runtime not found");
    }
    zip.addLocalFolder(
      path.join(TAURI_APP_DIR, webviewPath),
      path.basename(webviewPath),
    );
  }

  zip.addLocalFolder(configDir, ".config");

  const packageJson = JSON.parse(
    await Deno.readTextFile(path.join(cwd, "package.json")),
  );
  const version = packageJson.version;

  const zipFile = `Clash.Nyanpasu_${version}_${RUST_ARCH}${
    fixedWebview ? "_fixed-webview" : ""
  }_portable.zip`;
  zip.writeZip(zipFile);

  consola.success("create portable zip successfully");
}

resolvePortable().catch((err) => {
  consola.error(err);
  Deno.exit(1);
});
