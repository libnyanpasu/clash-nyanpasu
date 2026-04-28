import * as path from "jsr:@std/path";
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { Octokit } from "npm:octokit";
import { colorize, consola } from "./utils/logger.ts";
import { resolveUpdateLog } from "./updatelog.ts";

const GITHUB_PROXY = "https://gh-proxy.com/";
const UPDATE_TAG_NAME = "updater";
const UPDATE_JSON_FILE = "update.json";
const UPDATE_JSON_PROXY = "update-proxy.json";
const UPDATE_FIXED_WEBVIEW_FILE = "update-fixed-webview.json";
const UPDATE_FIXED_WEBVIEW_PROXY = "update-fixed-webview-proxy.json";
const UPDATE_RELEASE_BODY = Deno.env.get("RELEASE_BODY") ?? "";

const argv = parseArgs(Deno.args, {
  boolean: ["fixed-webview"],
  string: ["cache-path"],
  default: { "fixed-webview": false },
});

function getGithubUrl(url: string): string {
  return new URL(url.replace(/^https?:\/\//g, ""), GITHUB_PROXY).toString();
}

function getRepoContext() {
  const token = Deno.env.get("GITHUB_TOKEN");
  if (!token) throw new Error("GITHUB_TOKEN is required");
  const repoStr = Deno.env.get("GITHUB_REPOSITORY") ?? "";
  const [owner, repo] = repoStr.split("/");
  if (!owner || !repo) throw new Error("GITHUB_REPOSITORY must be owner/repo");
  return { token, owner, repo };
}

async function getSignature(url: string) {
  const response = await fetch(url, {
    method: "GET",
    headers: { "Content-Type": "application/octet-stream" },
  });
  return response.text();
}

async function saveToCache(fileName: string, content: string) {
  const cachePath = argv["cache-path"];
  if (!cachePath) return;
  try {
    await Deno.mkdir(cachePath, { recursive: true });
    const filePath = path.join(cachePath, fileName);
    await Deno.writeTextFile(filePath, content);
    consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`);
  } catch (err) {
    consola.error(`Failed to save cache file: ${err}`);
  }
}

async function resolveUpdater() {
  const { token, owner, repo } = getRepoContext();
  const github = new Octokit({ auth: token });
  const options = { owner, repo };

  const { data: tags } = await github.rest.repos.listTags({
    ...options,
    per_page: 10,
    page: 1,
  });

  const tag = (tags as Array<{ name: string }>).find((t) =>
    t.name.startsWith("v")
  );
  if (!tag) throw new Error("could not found the latest tag");
  consola.debug(colorize`latest tag: {gray.bold ${tag.name}}`);

  const { data: latestRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: tag.name,
  });

  let updateLog: string | null = null;
  try {
    updateLog = await resolveUpdateLog(tag.name);
  } catch (err) {
    consola.error(err);
  }

  const updateData = {
    name: tag.name,
    notes: UPDATE_RELEASE_BODY || updateLog || latestRelease.body,
    pub_date: new Date().toISOString(),
    platforms: {
      win64: { signature: "", url: "" },
      linux: { signature: "", url: "" },
      darwin: { signature: "", url: "" },
      "darwin-aarch64": { signature: "", url: "" },
      "darwin-intel": { signature: "", url: "" },
      "darwin-x86_64": { signature: "", url: "" },
      "linux-x86_64": { signature: "", url: "" },
      "windows-x86_64": { signature: "", url: "" },
      "windows-i686": { signature: "", url: "" },
      "windows-aarch64": { signature: "", url: "" },
    },
  };

  const promises = (
    latestRelease.assets as Array<{
      name: string;
      browser_download_url: string;
    }>
  ).map(async (asset) => {
    const { name, browser_download_url: browserDownloadUrl } = asset;

    function isMatch(name: string, extension: string, arch: string) {
      return (
        name.endsWith(extension) &&
        name.includes(arch) &&
        (argv["fixed-webview"]
          ? name.includes("fixed-webview")
          : !name.includes("fixed-webview"))
      );
    }

    if (isMatch(name, ".nsis.zip", "x64")) {
      updateData.platforms.win64.url = browserDownloadUrl;
      updateData.platforms["windows-x86_64"].url = browserDownloadUrl;
    }
    if (isMatch(name, ".nsis.zip.sig", "x64")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms.win64.signature = sig;
      updateData.platforms["windows-x86_64"].signature = sig;
    }
    if (isMatch(name, ".nsis.zip", "x86")) {
      updateData.platforms["windows-i686"].url = browserDownloadUrl;
    }
    if (isMatch(name, ".nsis.zip.sig", "x86")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms["windows-i686"].signature = sig;
    }
    if (isMatch(name, ".nsis.zip", "arm64")) {
      updateData.platforms["windows-aarch64"].url = browserDownloadUrl;
    }
    if (isMatch(name, ".nsis.zip.sig", "arm64")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms["windows-aarch64"].signature = sig;
    }
    if (name.endsWith(".app.tar.gz") && !name.includes("aarch")) {
      updateData.platforms.darwin.url = browserDownloadUrl;
      updateData.platforms["darwin-intel"].url = browserDownloadUrl;
      updateData.platforms["darwin-x86_64"].url = browserDownloadUrl;
    }
    if (name.endsWith(".app.tar.gz.sig") && !name.includes("aarch")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms.darwin.signature = sig;
      updateData.platforms["darwin-intel"].signature = sig;
      updateData.platforms["darwin-x86_64"].signature = sig;
    }
    if (name.endsWith("aarch64.app.tar.gz")) {
      updateData.platforms["darwin-aarch64"].url = browserDownloadUrl;
    }
    if (name.endsWith("aarch64.app.tar.gz.sig")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms["darwin-aarch64"].signature = sig;
    }
    if (name.endsWith(".AppImage.tar.gz")) {
      updateData.platforms.linux.url = browserDownloadUrl;
      updateData.platforms["linux-x86_64"].url = browserDownloadUrl;
    }
    if (name.endsWith(".AppImage.tar.gz.sig")) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms.linux.signature = sig;
      updateData.platforms["linux-x86_64"].signature = sig;
    }
  });

  await Promise.allSettled(promises);
  consola.info(updateData);

  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      consola.error(`failed to parse release for "${key}"`);
      delete updateData.platforms[key as keyof typeof updateData.platforms];
    }
  });

  const updateDataNew = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData;
  Object.entries(updateDataNew.platforms).forEach(([key, value]) => {
    if (value.url) {
      updateDataNew.platforms[key as keyof typeof updateData.platforms].url =
        getGithubUrl(value.url);
    } else {
      consola.error(`updateDataNew.platforms.${key} is null`);
    }
  });

  const { data: updateRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: UPDATE_TAG_NAME,
  });

  for (
    const asset of updateRelease.assets as Array<{
      name: string;
      id: number;
    }>
  ) {
    if (
      argv["fixed-webview"]
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      });
    }
    if (
      argv["fixed-webview"]
        ? asset.name === UPDATE_FIXED_WEBVIEW_PROXY
        : asset.name === UPDATE_JSON_PROXY
    ) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err: unknown) => {
          consola.error(err);
        });
    }
  }

  const mainFileName = argv["fixed-webview"]
    ? UPDATE_FIXED_WEBVIEW_FILE
    : UPDATE_JSON_FILE;
  const proxyFileName = argv["fixed-webview"]
    ? UPDATE_FIXED_WEBVIEW_PROXY
    : UPDATE_JSON_PROXY;
  const mainContent = JSON.stringify(updateData, null, 2);
  const proxyContent = JSON.stringify(updateDataNew, null, 2);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: mainFileName,
    data: mainContent,
  });
  await saveToCache(mainFileName, mainContent);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: proxyFileName,
    data: proxyContent,
  });
  await saveToCache(proxyFileName, proxyContent);
}

resolveUpdater().catch((err) => {
  consola.error(err);
});
