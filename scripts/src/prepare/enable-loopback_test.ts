import { assertEquals, assertRejects, assertThrows } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import {
  enableLoopbackInfo,
  getEnableLoopbackInfo,
  parseEnableLoopbackChecksum,
  parseEnableLoopbackVersion,
  verifyEnableLoopbackChecksum,
} from "./enable-loopback.ts";
import {
  resolveEnableLoopback,
  resolveEnableLoopbackBootstrapper,
} from "./resources.ts";
import type { downloadFile } from "./download.ts";

const BYTES = new TextEncoder().encode("abc");
const HASH = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

Deno.test("enable-loopback reads only the package version, including CRLF and prereleases", async () => {
  assertEquals(
    parseEnableLoopbackVersion(
      '[package]\r\nname = "tool"\r\nversion = "1.2.3-rc.1" # pin\r\n[dependencies]\r\n',
    ),
    "1.2.3-rc.1",
  );
  for (
    const manifest of [
      '[dependencies]\nversion = "9.9.9"',
      '[package]\nversion.workspace = true\n[dependencies]\nversion = "9.9.9"',
      '[package]\nversion = "latest"',
    ]
  ) {
    assertThrows(() => parseEnableLoopbackVersion(manifest));
  }
  const root = await Deno.makeTempDir();
  try {
    const dir = path.join(
      root,
      "backend/enable-loopback/crates/enable-loopback",
    );
    await Deno.mkdir(dir, { recursive: true });
    await Deno.writeTextFile(
      path.join(dir, "Cargo.toml"),
      "[package]\nversion = '2.3.4'\n",
    );
    assertEquals(
      await getEnableLoopbackInfo(root, "arm64"),
      enableLoopbackInfo("2.3.4", "arm64"),
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("enable-loopback release URLs follow both packaging targets", () => {
  for (const [arch, target] of [["x64", "x86_64"], ["arm64", "aarch64"]]) {
    const info = enableLoopbackInfo("0.1.0", arch);
    assertEquals(
      info.downloadURL,
      `https://github.com/libnyanpasu/enable-loopback/releases/download/v0.1.0/enableLoopback-${target}-pc-windows-msvc.exe`,
    );
    assertEquals(info.checksumURL, `${info.downloadURL}.sha256`);
    assertEquals(
      info.bootstrapper.downloadURL,
      `https://github.com/libnyanpasu/enable-loopback/releases/download/v0.1.0/Microsoft.WindowsAppRuntime.Bootstrap-${target}-pc-windows-msvc.dll`,
    );
    assertEquals(
      info.bootstrapper.checksumURL,
      `${info.bootstrapper.downloadURL}.sha256`,
    );
  }
  assertThrows(() => enableLoopbackInfo("0.1.0", "ia32"));
});

Deno.test("enable-loopback validates sha256sum format, filename and digest", async () => {
  const name = enableLoopbackInfo("0.1.0", "x64").assetName;
  assertEquals(
    parseEnableLoopbackChecksum(`${HASH.toUpperCase()}  ${name}\r\n`, name),
    HASH,
  );
  for (
    const checksum of [
      HASH,
      `${HASH}  other.exe`,
      `${HASH}  ${name}\n${HASH}  ${name}`,
      `bad  ${name}`,
    ]
  ) {
    assertThrows(() => parseEnableLoopbackChecksum(checksum, name));
  }
  await verifyEnableLoopbackChecksum(BYTES, `${HASH}  ${name}\n`, name);
  await assertRejects(
    () =>
      verifyEnableLoopbackChecksum(
        new Uint8Array([0]),
        `${HASH}  ${name}`,
        name,
      ),
    Error,
    "SHA-256 mismatch",
  );
});

Deno.test("enable-loopback installs verified bytes, replaces legacy cache and preserves it on mismatch", async () => {
  const root = await Deno.makeTempDir();
  const context = {
    resourcesDir: path.join(root, "resources"),
    sidecarDir: path.join(root, "sidecar"),
    tempRoot: path.join(root, "temp"),
    platform: "win32",
    arch: "x64",
  };
  const info = enableLoopbackInfo("0.1.0", "x64");
  let corrupt = false;
  let executableDownloads = 0;
  const download: typeof downloadFile = async (url, destination) => {
    if (url === info.checksumURL) {
      await Deno.writeTextFile(destination, `${HASH}  ${info.assetName}\n`);
    } else {
      assertEquals(url, info.downloadURL);
      executableDownloads++;
      await Deno.writeFile(destination, corrupt ? new Uint8Array([0]) : BYTES);
    }
    return { size: 3, speed: 1 };
  };
  try {
    await Deno.mkdir(context.resourcesDir);
    const target = path.join(context.resourcesDir, "enableLoopback.exe");
    await Deno.writeTextFile(target, "legacy tool");
    assertEquals(
      (await resolveEnableLoopback(context, info, undefined, download)).cached,
      false,
    );
    assertEquals(await Deno.readFile(target), BYTES);
    assertEquals(
      (await resolveEnableLoopback(context, info, undefined, download)).cached,
      true,
    );
    assertEquals(executableDownloads, 1);
    corrupt = true;
    await assertRejects(
      () => resolveEnableLoopback(context, info, { force: true }, download),
      Error,
      "SHA-256 mismatch",
    );
    assertEquals(await Deno.readFile(target), BYTES);
    await Deno.writeTextFile(target, "tampered");
    await assertRejects(
      () => resolveEnableLoopback(context, info, undefined, download),
      Error,
      "SHA-256 mismatch",
    );
    assertEquals(Array.from(Deno.readDirSync(context.tempRoot)), []);
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("enable-loopback installs the verified bootstrapper beside the executable", async () => {
  const root = await Deno.makeTempDir();
  const context = {
    resourcesDir: path.join(root, "resources"),
    sidecarDir: path.join(root, "sidecar"),
    tempRoot: path.join(root, "temp"),
    platform: "win32",
    arch: "arm64",
  };
  const info = enableLoopbackInfo("0.1.0", "arm64");
  const { bootstrapper } = info;
  let corrupt = false;
  const download: typeof downloadFile = async (url, destination) => {
    if (url === bootstrapper.checksumURL) {
      await Deno.writeTextFile(
        destination,
        `${HASH}  ${bootstrapper.assetName}\n`,
      );
    } else {
      assertEquals(url, bootstrapper.downloadURL);
      await Deno.writeFile(destination, corrupt ? new Uint8Array([0]) : BYTES);
    }
    return { size: 3, speed: 1 };
  };
  try {
    const target = path.join(
      context.resourcesDir,
      "Microsoft.WindowsAppRuntime.Bootstrap.dll",
    );
    const installed = await resolveEnableLoopbackBootstrapper(
      context,
      info,
      undefined,
      download,
    );
    assertEquals(installed.file, "Microsoft.WindowsAppRuntime.Bootstrap.dll");
    assertEquals(installed.cached, false);
    assertEquals(await Deno.readFile(target), BYTES);
    assertEquals(
      await Deno.readTextFile(`${target}.version`),
      `0.1.0/${bootstrapper.assetName}`,
    );
    assertEquals(
      (await resolveEnableLoopbackBootstrapper(
        context,
        info,
        undefined,
        download,
      )).cached,
      true,
    );
    corrupt = true;
    await assertRejects(
      () =>
        resolveEnableLoopbackBootstrapper(
          context,
          info,
          { force: true },
          download,
        ),
      Error,
      "SHA-256 mismatch",
    );
    assertEquals(await Deno.readFile(target), BYTES);
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});
