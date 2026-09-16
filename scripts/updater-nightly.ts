import * as path from "jsr:@std/path";
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { Octokit } from "npm:octokit";
import semver from "npm:semver";
import { z } from "npm:zod";
import { colorize, consola } from "./utils/logger.ts";
import {
  collectUpdaterPlatforms,
  UPDATER_TARGETS,
} from "./updater-platforms.ts";

const GITHUB_PROXY = "https://nyanpasu-script.majokeiko.com/";
const UPDATE_TAG_NAME = "updater";
const UPDATE_JSON_FILE = "update-nightly.json";
const UPDATE_JSON_PROXY = "update-nightly-proxy.json";

const argv = parseArgs(Deno.args, {
  string: ["cache-path"],
});

function getGithubUrl(url: string): string {
  // The proxy expects the GitHub path without the github.com host, e.g.
  // https://nyanpasu-script.majokeiko.com/<owner>/<repo>/releases/download/...
  return new URL(
    url.replace(/^https?:\/\/github\.com\//, ""),
    GITHUB_PROXY,
  ).toString();
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
  if (!response.ok) {
    throw new Error(`Failed to fetch signature: HTTP ${response.status}`);
  }
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
    platforms: await collectUpdaterPlatforms(
      latestPreRelease.assets,
      getSignature,
    ),
  };

  consola.info(updateData);
  for (const target of UPDATER_TARGETS) {
    if (!updateData.platforms[target]) {
      throw new Error(`failed to parse release for "${target}"`);
    }
  }

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
    if (asset.name === UPDATE_JSON_FILE) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      });
    }
    if (asset.name === UPDATE_JSON_PROXY) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err: unknown) => {
          consola.error(err);
        });
    }
  }

  const mainContent = JSON.stringify(updateData, null, 2);
  const proxyContent = JSON.stringify(updateDataNew, null, 2);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_FILE,
    data: mainContent,
  });
  await saveToCache(UPDATE_JSON_FILE, mainContent);

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_PROXY,
    data: proxyContent,
  });
  await saveToCache(UPDATE_JSON_PROXY, proxyContent);

  consola.success("updater files updated");
}

resolveUpdater().catch((err) => {
  consola.fatal(err);
  Deno.exit(1);
});
