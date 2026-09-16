import * as path from "jsr:@std/path";
import { parseArgs } from "jsr:@std/cli@1/parse-args";
import { Octokit } from "npm:octokit";
import { colorize, consola } from "./utils/logger.ts";
import {
  collectUpdaterPlatforms,
  UPDATER_TARGETS,
} from "./updater-platforms.ts";
import { resolveUpdateLog } from "./updatelog.ts";

const GITHUB_PROXY = "https://nyanpasu-script.majokeiko.com/";
const UPDATE_TAG_NAME = "updater";
const UPDATE_JSON_FILE = "update.json";
const UPDATE_JSON_PROXY = "update-proxy.json";
const UPDATE_RELEASE_BODY = Deno.env.get("RELEASE_BODY") ?? "";

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
    platforms: await collectUpdaterPlatforms(
      latestRelease.assets,
      getSignature,
    ),
  };

  consola.info(updateData);
  for (const target of UPDATER_TARGETS) {
    if (!updateData.platforms[target]) {
      consola.error(`failed to parse release for "${target}"`);
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
}

resolveUpdater().catch((err) => {
  consola.error(err);
  Deno.exit(1);
});
