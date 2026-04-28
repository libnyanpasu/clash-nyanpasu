import * as path from "jsr:@std/path";
import { exists } from "jsr:@std/fs";
import { Octokit } from "npm:octokit";
import { consola } from "./utils/logger.ts";

function getRepoContext() {
  const token = Deno.env.get("GITHUB_TOKEN");
  if (!token) throw new Error("GITHUB_TOKEN is required");
  const repoStr = Deno.env.get("GITHUB_REPOSITORY") ?? "";
  const [owner, repo] = repoStr.split("/");
  if (!owner || !repo) throw new Error("GITHUB_REPOSITORY must be owner/repo");
  return { token, owner, repo };
}

async function resolve() {
  if (!Deno.env.get("GITHUB_TOKEN")) {
    throw new Error("GITHUB_TOKEN is required");
  }
  if (!Deno.env.get("TAURI_SIGNING_PRIVATE_KEY")) {
    throw new Error("TAURI_SIGNING_PRIVATE_KEY is required");
  }
  if (!Deno.env.get("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")) {
    throw new Error("TAURI_SIGNING_PRIVATE_KEY_PASSWORD is required");
  }

  const { token, owner, repo } = getRepoContext();
  const packageJson = JSON.parse(
    await Deno.readTextFile(path.join(Deno.cwd(), "package.json")),
  );
  const version = packageJson.version;
  const tag = Deno.env.get("TAG_NAME") || `v${version}`;

  consola.info(`Upload to tag ${tag}`);

  const bundlePath = path.join(
    "backend/target/aarch64-apple-darwin/release/bundle",
  );
  const join = (p: string) => path.join(bundlePath, p);

  const appPathList = [
    join("macos/Clash Nyanpasu.aarch64.app.tar.gz"),
    join("macos/Clash Nyanpasu.aarch64.app.tar.gz.sig"),
  ];

  for (const appPath of appPathList) {
    if (await exists(appPath)) {
      await Deno.remove(appPath);
    }
  }

  await Deno.copyFile(
    join("macos/Clash Nyanpasu.app.tar.gz"),
    appPathList[0],
  );
  await Deno.copyFile(
    join("macos/Clash Nyanpasu.app.tar.gz.sig"),
    appPathList[1],
  );

  const github = new Octokit({ auth: token });

  const { data: release } = await github.rest.repos.getReleaseByTag({
    owner,
    repo,
    tag,
  });

  if (!release.id) throw new Error("failed to find the release");

  await uploadAssets(github, owner, repo, release.id, [
    join(`dmg/Clash Nyanpasu_${version}_aarch64.dmg`),
    ...appPathList,
  ]);
}

async function uploadAssets(
  github: Octokit,
  owner: string,
  repo: string,
  releaseId: number,
  assets: string[],
) {
  for (const assetPath of assets) {
    const stat = await Deno.stat(assetPath);
    const headers = {
      "content-type": "application/zip",
      "content-length": stat.size,
    };

    const ext = path.extname(assetPath);
    const basename = path.basename(assetPath);
    const filename = basename.slice(0, basename.length - ext.length);
    const assetName = assetPath.includes("target/debug")
      ? `${filename}-debug${ext}`
      : `${filename}${ext}`;

    consola.start(`Uploading ${assetName}...`);

    try {
      const data = await Deno.readFile(assetPath);
      await github.rest.repos.uploadReleaseAsset({
        headers,
        name: assetName,
        // @ts-ignore Uint8Array is accepted
        data,
        owner,
        repo,
        release_id: releaseId,
      });
      consola.success(`Uploaded ${assetName}`);
    } catch (error) {
      throw new Error(
        `Failed to upload release asset: ${
          error instanceof Error ? error.message : error
        }`,
      );
    }
  }
}

resolve();
