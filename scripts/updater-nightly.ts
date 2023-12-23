import { context, getOctokit } from "@actions/github";
import { execSync } from "child_process";
import { camelCase, upperFirst } from "lodash-es";
import fetch from "node-fetch";
import tauriNightly from "../backend/tauri/overrides/nightly.conf.json";
import { getGithubUrl } from "./utils";
import { consola } from "./utils/logger";
const UPDATE_TAG_NAME = "updater";
const UPDATE_JSON_FILE = "update-nightly.json";
const UPDATE_JSON_PROXY = "update-nightly-proxy.json";

/// generate update.json
/// upload to update tag's release asset
async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error("GITHUB_TOKEN is required");
  }
  consola.start("start to generate updater files");
  const options = {
    owner: context.repo.owner,
    repo: context.repo.repo,
  };
  const github = getOctokit(process.env.GITHUB_TOKEN);

  consola.debug("resolve latest pre-release files...");
  // latest pre-release tag
  const { data: latestPreRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: "pre-release",
  });
  const shortHash = await execSync(
    `git rev-parse --short ${latestPreRelease.target_commitish}`,
  )
    .toString()
    .replace("\n", "")
    .replace("\r", "");
  consola.info(`latest pre-release short hash: ${shortHash}`);
  const updateData = {
    name: `v${tauriNightly.package.version}-alpha+${shortHash}`,
    notes: "Nightly build. Full changes see commit history.",
    pub_date: new Date().toISOString(),
    platforms: {
      win64: { signature: "", url: "" }, // compatible with older formats
      linux: { signature: "", url: "" }, // compatible with older formats
      darwin: { signature: "", url: "" }, // compatible with older formats
      "darwin-aarch64": { signature: "", url: "" },
      "darwin-intel": { signature: "", url: "" },
      "darwin-x86_64": { signature: "", url: "" },
      "linux-x86_64": { signature: "", url: "" },
      "windows-x86_64": { signature: "", url: "" },
    },
  };

  const promises = latestPreRelease.assets.map(async (asset) => {
    const { name, browser_download_url } = asset;

    // win64 url
    if (name.endsWith(".msi.zip") && name.includes("en-US")) {
      updateData.platforms.win64.url = browser_download_url;
      updateData.platforms["windows-x86_64"].url = browser_download_url;
    }
    // win64 signature
    if (name.endsWith(".msi.zip.sig") && name.includes("en-US")) {
      const sig = await getSignature(browser_download_url);
      updateData.platforms.win64.signature = sig;
      updateData.platforms["windows-x86_64"].signature = sig;
    }

    // darwin url (intel)
    if (name.endsWith(".app.tar.gz") && !name.includes("aarch")) {
      updateData.platforms.darwin.url = browser_download_url;
      updateData.platforms["darwin-intel"].url = browser_download_url;
      updateData.platforms["darwin-x86_64"].url = browser_download_url;
    }
    // darwin signature (intel)
    if (name.endsWith(".app.tar.gz.sig") && !name.includes("aarch")) {
      const sig = await getSignature(browser_download_url);
      updateData.platforms.darwin.signature = sig;
      updateData.platforms["darwin-intel"].signature = sig;
      updateData.platforms["darwin-x86_64"].signature = sig;
    }

    // darwin url (aarch)
    if (name.endsWith("aarch64.app.tar.gz")) {
      updateData.platforms["darwin-aarch64"].url = browser_download_url;
    }
    // darwin signature (aarch)
    if (name.endsWith("aarch64.app.tar.gz.sig")) {
      const sig = await getSignature(browser_download_url);
      updateData.platforms["darwin-aarch64"].signature = sig;
    }

    // linux url
    if (name.endsWith(".AppImage.tar.gz")) {
      updateData.platforms.linux.url = browser_download_url;
      updateData.platforms["linux-x86_64"].url = browser_download_url;
    }
    // linux signature
    if (name.endsWith(".AppImage.tar.gz.sig")) {
      const sig = await getSignature(browser_download_url);
      updateData.platforms.linux.signature = sig;
      updateData.platforms["linux-x86_64"].signature = sig;
    }
  });

  await Promise.allSettled(promises);
  consola.info(updateData);

  consola.debug("generate updater metadata...");
  // maybe should test the signature as well
  // delete the null field
  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      consola.error(`failed to parse release for "${key}"`);
      delete updateData.platforms[key];
    }
  });

  // 生成一个代理github的更新文件
  // 使用 https://hub.fastgit.xyz/ 做github资源的加速
  const updateDataNew = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData;

  Object.entries(updateDataNew.platforms).forEach(([key, value]) => {
    if (value.url) {
      updateDataNew.platforms[key].url = getGithubUrl(value.url);
    } else {
      consola.error(`updateDataNew.platforms.${key} is null`);
    }
  });

  // update the update.json
  consola.debug("update updater files...");
  let updateRelease;
  try {
    const { data } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: UPDATE_TAG_NAME,
    });
    updateRelease = data;
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
    updateRelease = data;
  }

  // delete the old assets
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
        .catch((err) => {
          consola.error(err);
        }); // do not break the pipeline
    }
  }

  // upload new assets
  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_FILE,
    data: JSON.stringify(updateData, null, 2),
  });

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: UPDATE_JSON_PROXY,
    data: JSON.stringify(updateDataNew, null, 2),
  });
  consola.success("updater files updated");
}

// get the signature file content
async function getSignature(url) {
  const response = await fetch(url, {
    method: "GET",
    headers: { "Content-Type": "application/octet-stream" },
  });

  return response.text();
}

resolveUpdater().catch((err) => {
  consola.error(err);
});
