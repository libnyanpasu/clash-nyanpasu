import { execSync } from "node:child_process";
import os from "node:os";
import { colorize, consola } from "./utils/logger";
import { archCheck } from "./utils/arch-check";
import { Resolve } from "./utils/resolve";
import { printNyanpasu } from "./utils";
import { generateLatestVersion } from "generate-latest-version";

// force download
const FORCE = process.argv.includes("--force");

// cross platform build using
const ARCH = process.argv.includes("--arch")
  ? process.argv[process.argv.indexOf("--arch") + 1]
  : undefined;

// cross platform build support
const SIDECAR_HOST: string | undefined = process.argv.includes("--sidecar-host")
  ? process.argv[process.argv.indexOf("--sidecar-host") + 1]
  : execSync("rustc -vV")
      .toString()
      ?.match(/(?<=host: ).+(?=\s*)/g)?.[0];

if (!SIDECAR_HOST) {
  consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);
} else {
  consola.debug(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`);
}

const platform = process.platform;

const arch = ARCH ? ARCH : process.arch;

archCheck(platform, arch);

const resolve = new Resolve({
  platform,
  arch,
  sidecarHost: SIDECAR_HOST,
  force: FORCE,
});

const tasks: {
  name: string;
  func: () => Promise<void>;
  retry: number;
  winOnly?: boolean;
}[] = [
  { name: "clash", func: () => resolve.clash(), retry: 5 },
  { name: "mihomo", func: () => resolve.clashMeta(), retry: 5 },
  { name: "mihomo-alpha", func: () => resolve.clashMetaAlpha(), retry: 5 },
  { name: "clash-rs", func: () => resolve.clashRust(), retry: 5 },
  { name: "wintun", func: () => resolve.wintun(), retry: 5, winOnly: true },
  { name: "service", func: () => resolve.service(), retry: 5, winOnly: true },
  {
    name: "install",
    func: () => resolve.serviceInstall(),
    retry: 5,
    winOnly: true,
  },
  {
    name: "uninstall",
    func: () => resolve.serviceUninstall(),
    retry: 5,
    winOnly: true,
  },
  { name: "mmdb", func: () => resolve.mmdb(), retry: 5 },
  { name: "geoip", func: () => resolve.geoip(), retry: 5 },
  { name: "geosite", func: () => resolve.geosite(), retry: 5 },
  {
    name: "enableLoopback",
    func: () => resolve.enableLoopback(),
    retry: 5,
    winOnly: true,
  },
];

async function runTask() {
  const task = tasks.shift();

  if (!task) return;

  if (task.winOnly && process.platform !== "win32") return runTask();

  for (let i = 0; i < task.retry; i++) {
    try {
      await task.func();

      break;
    } catch (err) {
      consola.warn(`task::${task.name} try ${i} ==`, err);

      if (i === task.retry - 1) {
        consola.fatal(`task::${task.name} failed`, err);
      }
    }
  }

  return runTask();
}

consola.start("start check and download resources...");

const jobs = new Array(Math.ceil(os.cpus.length / 2) || 2)
  .fill(0)
  .map(() => runTask());

Promise.all(jobs).then(() => {
  printNyanpasu();

  consola.success("all resources download finished\n");

  const commands = [
    "pnpm dev - development with react dev tools",
    "pnpm dev:diff - deadlock development with react dev tools (recommend)",
    "pnpm tauri:diff - deadlock development",
  ];

  consola.log("  next command:\n");

  commands.forEach((text) => {
    consola.log(`    ${text}`);
  });
});
