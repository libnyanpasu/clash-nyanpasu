import path from "node:path";
import AdmZip from "adm-zip";
import fs from "fs-extra";
import { context, getOctokit } from "@actions/github";
import packageJson from "../package.json";
import { colorize, consola } from "./utils/logger";

/// Script for ci
/// 打包绿色版/便携版 (only Windows)
async function resolvePortable() {
  if (process.platform !== "win32") return;

  const releaseDir = path.join("backend/target/release");
  const configDir = path.join(releaseDir, ".config");

  if (!(await fs.pathExists(releaseDir))) {
    throw new Error("could not found the release dir");
  }

  await fs.ensureDir(configDir);
  await fs.createFile(path.join(configDir, "PORTABLE"));

  const zip = new AdmZip();

  zip.addLocalFile(path.join(releaseDir, "Clash Nyanpasu.exe"));
  zip.addLocalFile(path.join(releaseDir, "clash.exe"));
  zip.addLocalFile(path.join(releaseDir, "mihomo.exe"));
  zip.addLocalFile(path.join(releaseDir, "mihomo-alpha.exe"));
  zip.addLocalFile(path.join(releaseDir, "nyanpasu-service.exe"));
  zip.addLocalFile(path.join(releaseDir, "clash-rs.exe"));
  zip.addLocalFolder(path.join(releaseDir, "resources"), "resources");
  zip.addLocalFolder(configDir, ".config");

  const { version } = packageJson;

  const zipFile = `Clash.Nyanpasu_${version}_x64_portable.zip`;
  zip.writeZip(zipFile);

  consola.success("create portable zip successfully");

  // push release assets
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error("GITHUB_TOKEN is required");
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo };
  const github = getOctokit(process.env.GITHUB_TOKEN);

  consola.info("upload to ", process.env.TAG_NAME || `v${version}`);

  const { data: release } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: process.env.TAG_NAME || `v${version}`,
  });

  consola.debug(colorize`releaseName: {green ${release.name}}`);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: release.id,
    name: zipFile,
    // @ts-expect-error data is Buffer should work fine
    data: zip.toBuffer(),
  });
}

resolvePortable().catch((err) => {
  consola.error(err);
  process.exit(1);
});
