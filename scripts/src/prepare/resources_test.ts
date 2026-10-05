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

async function fileSha256(filePath: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    await Deno.readFile(filePath),
  );
  return Array.from(
    new Uint8Array(digest),
    (byte) => byte.toString(16).padStart(2, "0"),
  ).join("");
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

Deno.test("Meow Alpha rejects an archive digest mismatch without replacing cache", async () => {
  const root = await Deno.makeTempDir();
  const target = foreignExecutableTarget();
  const context = resolverContext(root, target);
  const assetName = "meow-alpha-3c27aca-x86_64-unknown-linux-musl.tar.gz";
  const tempFile = path.join(context.tempRoot, "meow-alpha", assetName);
  await Deno.mkdir(path.dirname(tempFile), { recursive: true });
  await createMeowTarball(path.dirname(tempFile), assetName, target.platform);
  await Deno.mkdir(context.sidecarDir, { recursive: true });
  const sidecarPath = path.join(
    context.sidecarDir,
    "meow-alpha-x86_64-unknown-linux-musl",
  );
  await Deno.writeTextFile(sidecarPath, "previous-good-binary");
  await Deno.writeTextFile(`${sidecarPath}.version`, "alpha-3c27aca");
  const info = {
    ...meowInfo("meow-alpha", "alpha-3c27aca", assetName),
    sha256: "0".repeat(64),
  };

  try {
    await assertRejects(
      () => resolveSidecar(context, info, { force: true }),
      Error,
      "SHA-256 mismatch",
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

Deno.test("Meow Alpha version mismatch keeps the previous binary and stamp", async () => {
  const root = await Deno.makeTempDir();
  const target = nativeExecutableTarget();
  const context = resolverContext(root, target);
  const assetName = `meow-alpha-3c27aca-${target.platform}-native.tar.gz`;
  const tempDir = path.join(context.tempRoot, "meow-alpha");
  await Deno.mkdir(tempDir, { recursive: true });
  const archivePath = await createMeowTarball(
    tempDir,
    assetName,
    target.platform,
  );
  const sha256 = await fileSha256(archivePath);
  await Deno.mkdir(context.sidecarDir, { recursive: true });
  const sidecarPath = path.join(context.sidecarDir, "meow-alpha-native");
  await Deno.writeTextFile(sidecarPath, "previous-good-binary");
  await Deno.writeTextFile(`${sidecarPath}.version`, "alpha-3c27aca");
  const info = {
    ...meowInfo("meow-alpha", "alpha-3c27aca", assetName),
    targetFile: "meow-alpha-native",
    sha256,
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
