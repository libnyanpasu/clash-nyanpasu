import { assertEquals, assertRejects, assertThrows } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import {
  assertMeowVersion,
  resolveSidecar,
  type ResourceResolverOptions,
} from "./resources.ts";
import { normalizeArch, normalizePlatform } from "./platform.ts";
import type { BinInfo } from "./types.ts";

Deno.test("Meow version validation accepts matching stable and alpha output", () => {
  assertMeowVersion("Meow Meta 0.22.0", "v0.22.0");
  assertMeowVersion("Meow Meta 0.22.0-alpha+3c27aca", "alpha-3c27aca");
  assertThrows(
    () => assertMeowVersion("Meow Meta 0.22.0-alpha+deadbee", "alpha-3c27aca"),
    Error,
    "unexpected Meow alpha version",
  );
});

async function createMeowTarball(
  tempRoot: string,
  name: string,
  platform: string,
): Promise<string> {
  const stageDir = path.join(tempRoot, "fixture");
  const assetDir = path.join(stageDir, name.replace(/\.tar\.gz$/, ""));
  await Deno.mkdir(assetDir, { recursive: true });
  const magic = platform === "linux"
    ? new Uint8Array([0x7f, 0x45, 0x4c, 0x46])
    : platform === "win32"
    ? new Uint8Array([0x4d, 0x5a, 0x90, 0x00])
    : new Uint8Array([0xcf, 0xfa, 0xed, 0xfe]);
  await Deno.writeFile(
    path.join(assetDir, "meow"),
    new Uint8Array([...magic, ...new TextEncoder().encode("fixture")]),
    { mode: 0o755 },
  );

  const archive = path.join(tempRoot, name);
  const result = await new Deno.Command("tar", {
    args: ["-czf", archive, "-C", stageDir, path.basename(assetDir)],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (result.code !== 0) {
    throw new Error(new TextDecoder().decode(result.stderr));
  }
  return archive;
}

function foreignExecutableTarget(): { platform: string; arch: string } {
  const platform = Deno.build.os === "linux" ? "darwin" : "linux";
  return { platform, arch: "x64" };
}

function nativeExecutableTarget(): { platform: string; arch: string } {
  return {
    platform: normalizePlatform(Deno.build.os),
    arch: normalizeArch(Deno.build.arch),
  };
}

function resolverContext(
  root: string,
  target: { platform: string; arch: string },
): ResourceResolverOptions {
  return {
    resourcesDir: path.join(root, "resources"),
    sidecarDir: path.join(root, "sidecar"),
    tempRoot: path.join(root, "temp"),
    ...target,
  };
}

function meowInfo(name: string, version: string, assetName: string): BinInfo {
  return {
    name,
    version,
    targetFile: `${name}-x86_64-unknown-linux-musl`,
    tmpFile: assetName,
    exeFile: `${assetName.replace(/\.tar\.gz$/, "")}/meow`,
    downloadURL: "unused-because-the-test-seeds-the-cache",
  };
}

Deno.test("Meow repairs an unstamped legacy sidecar from the archive executable", async () => {
  const root = await Deno.makeTempDir();
  const target = foreignExecutableTarget();
  const context = resolverContext(root, target);
  const assetName = "meow-v0.22.0-x86_64-unknown-linux-musl.tar.gz";
  const tempFile = path.join(context.tempRoot, "meow", assetName);
  await Deno.mkdir(path.dirname(tempFile), { recursive: true });
  await createMeowTarball(path.dirname(tempFile), assetName, target.platform);
  await Deno.mkdir(context.sidecarDir, { recursive: true });
  const sidecarPath = path.join(
    context.sidecarDir,
    "meow-x86_64-unknown-linux-musl",
  );
  await Deno.writeFile(sidecarPath, new Uint8Array([0x1f, 0x8b, 0x08]));

  try {
    const result = await resolveSidecar(
      context,
      meowInfo("meow", "v0.22.0", assetName),
    );
    assertEquals(result.cached, false);
    assertEquals(
      Array.from((await Deno.readFile(sidecarPath)).slice(0, 4)),
      target.platform === "linux"
        ? [0x7f, 0x45, 0x4c, 0x46]
        : [0xcf, 0xfa, 0xed, 0xfe],
    );
    assertEquals(
      (await Deno.readTextFile(`${sidecarPath}.version`)).trim(),
      "v0.22.0",
    );
    assertEquals(
      (await resolveSidecar(context, meowInfo("meow", "v0.22.0", assetName)))
        .cached,
      true,
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("Meow Alpha version mismatch keeps the previous binary and stamp", async () => {
  const root = await Deno.makeTempDir();
  const target = nativeExecutableTarget();
  const context = resolverContext(root, target);
  const assetName = `meow-alpha-3c27aca-${target.platform}-native.tar.gz`;
  const tempDir = path.join(context.tempRoot, "meow-alpha");
  await Deno.mkdir(tempDir, { recursive: true });
  await createMeowTarball(
    tempDir,
    assetName,
    target.platform,
  );
  await Deno.mkdir(context.sidecarDir, { recursive: true });
  const sidecarPath = path.join(context.sidecarDir, "meow-alpha-native");
  await Deno.writeTextFile(sidecarPath, "previous-good-binary");
  await Deno.writeTextFile(`${sidecarPath}.version`, "alpha-3c27aca");
  const info = {
    ...meowInfo("meow-alpha", "alpha-3c27aca", assetName),
    targetFile: "meow-alpha-native",
  };
  context.runMeowVersion = async () => "Meow Meta 0.22.0-alpha+deadbee";

  try {
    await assertRejects(
      () => resolveSidecar(context, info, { force: true }),
      Error,
      "unexpected Meow alpha version",
    );
    assertEquals(await Deno.readTextFile(sidecarPath), "previous-good-binary");
    assertEquals(
      await Deno.readTextFile(`${sidecarPath}.version`),
      "alpha-3c27aca",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("Sidecar refreshes a gone alpha asset from the live release", async () => {
  const root = await Deno.makeTempDir();
  const context = resolverContext(root, foreignExecutableTarget());
  const originalFetch = globalThis.fetch;
  let refreshCalls = 0;
  globalThis.fetch = (input) => {
    const url = String(input);
    if (url === "https://example.test/stale-alpha") {
      return Promise.resolve(new Response("gone", { status: 404 }));
    }
    if (url === "https://example.test/live-alpha") {
      return Promise.resolve(
        new Response(new Uint8Array([0x01, 0x02, 0x03])),
      );
    }
    return Promise.reject(new Error(`unexpected fetch: ${url}`));
  };

  const targetFile = "clash-rs-alpha-fixture";
  const pinned: BinInfo = {
    name: "clash-rs-alpha",
    version: "alpha-stale",
    targetFile,
    tmpFile: "clash-rs-alpha-stale",
    exeFile: "clash-rs-alpha-stale",
    downloadURL: "https://example.test/stale-alpha",
    refresh: () => {
      refreshCalls++;
      return Promise.resolve({
        ...pinned,
        version: "alpha-live",
        tmpFile: "clash-rs-alpha-live",
        exeFile: "clash-rs-alpha-live",
        downloadURL: "https://example.test/live-alpha",
      });
    },
  };

  try {
    const result = await resolveSidecar(context, pinned);
    assertEquals(refreshCalls, 1);
    assertEquals(result.cached, false);
    assertEquals(result.version, "alpha-live");
    assertEquals(result.size, 3);
    assertEquals(
      await Deno.readFile(path.join(context.sidecarDir, targetFile)),
      new Uint8Array([0x01, 0x02, 0x03]),
    );
  } finally {
    globalThis.fetch = originalFetch;
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("Sidecar keeps the pinned asset while it is still available", async () => {
  const root = await Deno.makeTempDir();
  const context = resolverContext(root, foreignExecutableTarget());
  const originalFetch = globalThis.fetch;
  let refreshCalls = 0;
  globalThis.fetch = () =>
    Promise.resolve(new Response(new Uint8Array([0x04, 0x05])));

  const pinned: BinInfo = {
    name: "clash-rs-alpha",
    version: "alpha-pinned",
    targetFile: "clash-rs-alpha-pinned",
    tmpFile: "clash-rs-alpha-pinned",
    exeFile: "clash-rs-alpha-pinned",
    downloadURL: "https://example.test/pinned-alpha",
    refresh: () => {
      refreshCalls++;
      return Promise.reject(new Error("refresh must not run"));
    },
  };

  try {
    const result = await resolveSidecar(context, pinned);
    assertEquals(refreshCalls, 0);
    assertEquals(result.version, "alpha-pinned");
  } finally {
    globalThis.fetch = originalFetch;
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("Sidecar reports the pinned download error when refresh fails", async () => {
  const root = await Deno.makeTempDir();
  const context = resolverContext(root, foreignExecutableTarget());
  const originalFetch = globalThis.fetch;
  let refreshCalls = 0;
  globalThis.fetch = () =>
    Promise.resolve(new Response("gone", { status: 404 }));

  const pinned: BinInfo = {
    name: "meow-alpha",
    version: "alpha-stale",
    targetFile: "meow-alpha-fixture",
    tmpFile: "meow-alpha-stale.tar.gz",
    exeFile: "meow-alpha-stale/meow",
    downloadURL: "https://example.test/stale-meow-alpha",
    refresh: () => {
      refreshCalls++;
      return Promise.reject(new Error("live lookup failed"));
    },
  };

  try {
    await assertRejects(
      () => resolveSidecar(context, pinned),
      Error,
      "download failed",
    );
    assertEquals(refreshCalls, 1);
  } finally {
    globalThis.fetch = originalFetch;
    await Deno.remove(root, { recursive: true });
  }
});
