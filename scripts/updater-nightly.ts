import * as path from "jsr:@std/path";
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { Octokit } from "npm:octokit";
import semver from "npm:semver";
import { z } from "npm:zod";
import { colorize, consola } from "./utils/logger.ts";

const GITHUB_PROXY = "https://gh-proxy.com/";
const UPDATE_TAG_NAME = "updater";
const UPDATE_JSON_FILE = "update-nightly.json";
const UPDATE_JSON_PROXY = "update-nightly-proxy.json";
const UPDATE_FIXED_WEBVIEW_FILE = "update-nightly-fixed-webview.json";
const UPDATE_FIXED_WEBVIEW_PROXY = "update-nightly-fixed-webview-proxy.json";

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
    throw new Error(`Failed to save cache file: ${err}`);
  }
}

function upperFirst(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function camelCase(s: string) {
  return s.replace(/-(\w)/g, (_, c: string) => c.toUpperCase());
}

async function resolveUpdater() {
  if (!Deno.env.get("GITHUB_TOKEN")) {
    throw new Error("GITHUB_TOKEN is required");
  }
  consola.start("start to generate updater files");

  const { token, owner, repo } = getRepoContext();
  const github = new Octokit({ auth: token });
  const options = { owner, repo };

  const tauriNightlyPath = path.join(
    Deno.cwd(),
    "backend/tauri/overrides/nightly.conf.json",
  );
  const tauriNightly = JSON.parse(await Deno.readTextFile(tauriNightlyPath));

  consola.debug("resolve latest pre-release files...");
  const { data: latestPreRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: "pre-release",
  });

  let shortHash = "";
  const latestContent = (
    latestPreRelease.assets as Array<{
      name: string;
      browser_download_url: string;
    }>
  ).find((o) => o.name === "latest.json");

  if (latestContent) {
    const schema = z.object({ version: z.string().min(1) });
    const latest = schema.parse(
      await fetch(latestContent.browser_download_url).then((res) => res.json()),
    );
    const version = semver.parse(latest.version);
    if (version && version.build.length > 0) {
      console.log(version);
      shortHash = version.build[0];
    }
  }

  if (!shortHash) {
    const gitResult = await new Deno.Command("git", {
      args: ["rev-parse", "--short", "pre-release"],
      stdout: "piped",
    }).output();
    shortHash = new TextDecoder().decode(gitResult.stdout).trim().slice(0, 7);
  }

  consola.info(`latest pre-release short hash: ${shortHash}`);

  const updateData = {
    name: `v${tauriNightly.version}-alpha+${shortHash}`,
    notes: "Nightly build. Full changes see commit history.",
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
    latestPreRelease.assets as Array<{
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

  consola.debug("generate updater metadata...");
  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      throw new Error(`failed to parse release for "${key}"`);
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

  consola.debug("update updater files...");
  let updateRelease: {
    id: number;
    assets: Array<{ name: string; id: number }>;
  };
  try {
    const { data } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: UPDATE_TAG_NAME,
    });
    updateRelease = data as typeof updateRelease;
  } catch (err) {
    consola.error(err);
    consola.error("failed to get release by tag, create one");
    const { data } = await github.rest.repos.createRelease({
      ...options,
      tag_name: UPDATE_TAG_NAME,
      name: upperFirst(camelCase(UPDATE_TAG_NAME)),
      body: "files for programs to check for updates",
      prerelease: true,
    });
    updateRelease = data as typeof updateRelease;
  }

  for (const asset of updateRelease.assets) {
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

  consola.success("updater files updated");
}

resolveUpdater().catch((err) => {
  consola.fatal(err);
  Deno.exit(1);
});
