import * as path from "jsr:@std/path";
import { ensureDir, exists } from "jsr:@std/fs";
import AdmZip from "npm:adm-zip";
import { Octokit } from "npm:octokit";
import { colorize, consola } from "./utils/logger.ts";

function getRepoContext() {
  const token = Deno.env.get("GITHUB_TOKEN");
  if (!token) throw new Error("GITHUB_TOKEN is required");
  const repoStr = Deno.env.get("GITHUB_REPOSITORY") ?? "";
  const [owner, repo] = repoStr.split("/");
  if (!owner || !repo) throw new Error("GITHUB_REPOSITORY must be owner/repo");
  return { token, owner, repo };
}

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

  if (!Deno.env.get("GITHUB_TOKEN")) {
    throw new Error("GITHUB_TOKEN is required");
  }

  const { token, owner, repo } = getRepoContext();
  const github = new Octokit({ auth: token });
  const options = { owner, repo };

  consola.info("upload to ", Deno.env.get("TAG_NAME") || `v${version}`);

  const { data: release } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: Deno.env.get("TAG_NAME") || `v${version}`,
  });

  consola.debug(colorize`releaseName: {green ${release.name}}`);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: release.id,
    name: zipFile,
    // @ts-ignore Buffer-compatible Uint8Array
    data: zip.toBuffer(),
  });
}

resolvePortable().catch((err) => {
  consola.error(err);
  Deno.exit(1);
});
