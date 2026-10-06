import { parseArgs } from "jsr:@std/cli@1/parse-args";
import * as path from "jsr:@std/path";
// @ts-types="npm:@types/figlet"
import figlet from "npm:figlet";
import { colorize, consola } from "../shared/logger.ts";
import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import { createBinaryResolvers } from "./binaries.ts";
import { normalizeArch, normalizePlatform } from "./platform.ts";
import {
  formatProgressSize,
  formatResolveInfo,
  formatSpeed,
  ProgressRenderer,
} from "./progress.ts";
import { resolveResource, resolveSidecar, resolveWintun } from "./resources.ts";
import { normalizeVersion } from "./versions.ts";
import type {
  DownloadProgress,
  ResolveInfo,
  VersionManifest,
} from "./types.ts";

interface CheckArgs {
  force?: boolean;
  arch?: string;
  "sidecar-host"?: string;
}

interface Task {
  name: string;
  version?: string;
  func: (
    onProgress?: (progress: DownloadProgress) => void,
  ) => Promise<ResolveInfo>;
  retry: number;
  winOnly?: boolean;
}

async function runTask(
  queue: Task[],
  progress: ProgressRenderer,
): Promise<void> {
  const task = queue.shift();
  if (!task) return;

  for (let attempt = 0; attempt < task.retry; attempt++) {
    try {
      progress.update(
        task.name,
        "Pulling",
        task.version ? { version: task.version } : {},
      );
      const info = await task.func((download) => {
        progress.update(task.name, "Pulling", {
          version: normalizeVersion(download.version) ?? task.version,
          size: formatProgressSize(download.downloaded, download.total),
          speed: formatSpeed(download.speed),
        });
      });
      progress.update(task.name, "Done", formatResolveInfo(info));
      break;
    } catch (err) {
      if (attempt === task.retry - 1) {
        progress.update(task.name, "Failed");
        progress.finish();
        consola.fatal(`task::${task.name} failed`, err);
        Deno.exit(1);
      }
      progress.update(task.name, "Retrying", {
        note: `${attempt + 1}/${task.retry}`,
        version: task.version,
      });
    }
  }

  return runTask(queue, progress);
}

async function resolveSidecarHost(
  hostOverride?: string,
): Promise<string | undefined> {
  if (hostOverride) return hostOverride;
  const cmd = new Deno.Command("rustc", { args: ["-vV"], stdout: "piped" });
  const { stdout } = await cmd.output();
  const output = new TextDecoder().decode(stdout);
  return output.match(/host: (.+)/)?.[1]?.trim();
}

async function main(args = parseArgs(Deno.args, {
  boolean: ["force"],
  string: ["arch", "sidecar-host"],
}) as CheckArgs): Promise<void> {
  const force = args.force;
  const arch = args.arch ?? normalizeArch(Deno.build.arch);
  const platform = normalizePlatform(Deno.build.os);
  const debug = Deno.env.get("LOG_LEVEL") !== undefined;
  const debugLog = (message: string) => {
    if (debug) consola.debug(message);
  };

  const sidecarHost = await resolveSidecarHost(args["sidecar-host"]);
  if (!sidecarHost) {
    consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);
    Deno.exit(1);
  }

  debugLog(colorize`sidecar-host {yellow ${sidecarHost}}`);
  debugLog(colorize`platform {yellow ${platform}}`);
  debugLog(colorize`arch {yellow ${arch}}`);

  const versionManifest = JSON.parse(
    await Deno.readTextFile(path.join(WORKSPACE_ROOT, "manifest/version.json")),
  ) as VersionManifest;
  const binary = createBinaryResolvers({
    versionManifest,
    sidecarHost,
    platform,
    arch,
    workspaceRoot: WORKSPACE_ROOT,
    debugLog,
  });
  const resourceContext = {
    resourcesDir: path.join(WORKSPACE_ROOT, "backend/tauri/resources"),
    sidecarDir: path.join(WORKSPACE_ROOT, "backend/tauri/sidecar"),
    tempRoot: path.join(WORKSPACE_ROOT, "node_modules/.verge"),
    platform,
    arch,
    debugLog,
  };
  const tasks: Task[] = [
    {
      name: "clash",
      version: versionManifest.latest.clash_premium,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clash(), { force, onProgress }),
      retry: 5,
    },
    {
      name: "mihomo",
      version: versionManifest.latest.mihomo,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.mihomo(), { force, onProgress }),
      retry: 5,
    },
    {
      name: "mihomo-alpha",
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.mihomoAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: "clash-rs",
      version: versionManifest.latest.clash_rs,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clashRs(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: "clash-rs-alpha",
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clashRsAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: "meow",
      version: versionManifest.latest.meow,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.meow(), { force, onProgress }),
      retry: 5,
    },
    {
      name: "meow-alpha",
      version: versionManifest.latest.meow_alpha,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.meowAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: "wintun",
      func: (onProgress) =>
        resolveWintun(resourceContext, force ?? false, onProgress),
      retry: 5,
      winOnly: true,
    },
    {
      name: "nyanpasu-service",
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.nyanpasuService(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: "mmdb",
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: "Country.mmdb",
            downloadURL:
              "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb",
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: "geoip",
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: "geoip.dat",
            downloadURL:
              "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: "geosite",
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: "geosite.dat",
            downloadURL:
              "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: "enableLoopback",
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: "enableLoopback.exe",
            downloadURL:
              "https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe",
          },
          { force, onProgress },
        ),
      retry: 5,
      winOnly: true,
    },
  ];

  const activeTasks = tasks.filter((task) =>
    !task.winOnly || platform === "win32"
  );
  const progress = new ProgressRenderer(
    activeTasks.map((task) => task.name),
    consola,
    Deno.stdout.isTerminal() && !Deno.env.get("CI") && !debug,
    (text) => Deno.stdout.writeSync(new TextEncoder().encode(text)),
  );
  progress.start();

  const concurrency = Math.ceil(navigator.hardwareConcurrency / 2) || 2;
  const queue = [...activeTasks];
  await Promise.all(
    Array.from({ length: concurrency }, () => runTask(queue, progress)),
  );
  progress.finish();

  console.log(figlet.textSync("Clash Nyanpasu", { whitespaceBreak: true }));
  consola.success("all resources download finished\n");
  consola.log("  next command:\n");
  consola.log("    pnpm dev - development");
  consola.log("    pnpm dev:diff - deadlock development (recommend)");
}

if (import.meta.main) await main();
