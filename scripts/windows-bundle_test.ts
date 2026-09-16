import { assert, assertEquals, assertRejects } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import { ensureDir } from "jsr:@std/fs";
import AdmZip from "npm:adm-zip";
import { createPortableArchive } from "./portable.ts";
const FIXED_WEBVIEW_DIR = "WebView2";

const tauriSource = new URL("../backend/tauri/", import.meta.url);

// Tauri merges config using JSON Merge Patch: objects merge, arrays replace.
function mergeConfig(...configs: Record<string, unknown>[]) {
  const result: Record<string, unknown> = {};
  for (const config of configs) {
    for (const [key, value] of Object.entries(config)) {
      if (value === null) {
        delete result[key];
      } else if (typeof value === "object" && !Array.isArray(value)) {
        const previous = result[key];
        result[key] = mergeConfig(
          previous && typeof previous === "object" && !Array.isArray(previous)
            ? previous as Record<string, unknown>
            : {},
          value as Record<string, unknown>,
        );
      } else {
        result[key] = value;
      }
    }
  }
  return result;
}

async function withFixture(
  run: (directory: string) => Promise<void>,
) {
  const directory = await Deno.makeTempDir({ prefix: "nyanpasu-bundle-test-" });
  try {
    await ensureDir(path.join(directory, "overrides"));
    for (
      const file of [
        "tauri.conf.json",
        "tauri.windows.conf.json",
        "overrides/fixed-webview2.conf.json",
        "overrides/nightly.conf.json",
      ]
    ) {
      await Deno.copyFile(
        new URL(file, tauriSource),
        path.join(directory, file),
      );
    }
    await run(directory);
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
}

async function addRuntime(directory: string) {
  const runtime = path.join(directory, FIXED_WEBVIEW_DIR);
  await ensureDir(path.join(runtime, "locales"));
  await Deno.writeTextFile(path.join(runtime, "msedgewebview2.exe"), "runtime");
  await Deno.writeTextFile(path.join(runtime, "locales/en-US.pak"), "locale");
}

Deno.test("fixed packaging changes only WebView2 mode and keeps shared update URLs", async () => {
  await withFixture(async (directory) => {
    const read = async (file: string) =>
      JSON.parse(await Deno.readTextFile(path.join(directory, file)));
    const base = await read("tauri.conf.json");
    const windows = await read("tauri.windows.conf.json");
    const fixed = await read("overrides/fixed-webview2.conf.json");
    const nightly = mergeConfig(
      base,
      await read("overrides/nightly.conf.json"),
    );
    for (const channel of [{}, nightly]) {
      const compiled = mergeConfig(base, windows, channel);
      const bundled = mergeConfig(compiled, fixed);
      assertEquals(
        bundled,
        mergeConfig(compiled, {
          $schema: fixed.$schema,
          bundle: {
            windows: {
              webviewInstallMode: {
                type: "fixedRuntime",
                path: FIXED_WEBVIEW_DIR,
              },
            },
          },
        }),
      );
    }
  });
});

Deno.test("shared release preparation preserves Linux and macOS config without Windows assets", async () => {
  await withFixture(async (directory) => {
    const tauriDir = path.join(directory, "backend/tauri");
    await ensureDir(tauriDir);
    const manifest = '{"name":"release-fixture","version":"1.2.3"}\n';
    await Deno.writeTextFile(path.join(directory, "package.json"), manifest);
    for (const targets of [["deb", "appimage", "rpm"], ["app", "dmg"]]) {
      const config = {
        version: "1.2.3",
        bundle: { targets, resources: ["resources"] },
        plugins: {
          updater: { endpoints: ["https://example.com/update.json"] },
        },
      };
      await Deno.writeTextFile(
        path.join(tauriDir, "tauri.conf.json"),
        JSON.stringify(config),
      );
      const result = await new Deno.Command(Deno.execPath(), {
        args: [
          "run",
          "--config",
          path.fromFileUrl(new URL("./deno.jsonc", import.meta.url)),
          "-A",
          path.fromFileUrl(new URL("./prepare-release.ts", import.meta.url)),
        ],
        cwd: directory,
        stdout: "piped",
        stderr: "piped",
      }).output();
      assert(result.success, new TextDecoder().decode(result.stderr));
      assertEquals(
        JSON.parse(
          await Deno.readTextFile(path.join(tauriDir, "tauri.conf.json")),
        ),
        config,
      );
      assertEquals(
        await Deno.readTextFile(path.join(directory, "package.json")),
        manifest,
      );
      assertEquals(
        Array.from(Deno.readDirSync(tauriDir)).map((entry) => entry.name),
        ["tauri.conf.json"],
      );
    }
  });
});

Deno.test("portable archives reuse the application and differ only by bundled runtime", async () => {
  await withFixture(async (directory) => {
    await addRuntime(directory);
    const buildDir = path.join(directory, "target/release");
    await ensureDir(path.join(buildDir, "resources"));
    const binaryNames = [
      "Clash Nyanpasu.exe",
      "clash.exe",
      "mihomo.exe",
      "mihomo-alpha.exe",
      "nyanpasu-service.exe",
      "clash-rs.exe",
      "clash-rs-alpha.exe",
    ];
    for (const name of binaryNames) {
      await Deno.writeTextFile(path.join(buildDir, name), `binary:${name}`);
    }
    await Deno.writeTextFile(
      path.join(buildDir, "resources/geoip.dat"),
      "geoip",
    );
    for (
      const variant of ["fixed-webview", "standard", "fixed-webview"] as const
    ) {
      const archive = await createPortableArchive(buildDir, directory, variant);
      // Round-trip the actual ZIP bytes, not just the in-memory entry list.
      const zip = new AdmZip(archive.toBuffer());
      const expectedFiles = [
        ...binaryNames,
        ".config/PORTABLE",
        "resources/geoip.dat",
      ];
      if (variant === "fixed-webview") {
        expectedFiles.push(
          "WebView2/msedgewebview2.exe",
          "WebView2/locales/en-US.pak",
        );
      }
      assertEquals(
        zip.getEntries().filter((entry) => !entry.isDirectory).map((entry) =>
          entry.entryName
        ).sort(),
        expectedFiles.sort(),
      );
      assert(zip.getEntry(".config/PORTABLE"));
      assertEquals(zip.readAsText("resources/geoip.dat"), "geoip");
      for (const name of binaryNames) {
        assertEquals(zip.readAsText(name), `binary:${name}`);
      }
      const runtimeFiles = zip.getEntries().filter((entry) =>
        entry.entryName.startsWith(`${FIXED_WEBVIEW_DIR}/`) &&
        !entry.isDirectory
      ).map((entry) => entry.entryName).sort();
      assertEquals(
        runtimeFiles,
        variant === "fixed-webview"
          ? ["WebView2/locales/en-US.pak", "WebView2/msedgewebview2.exe"]
          : [],
      );
    }
    await Deno.remove(path.join(directory, FIXED_WEBVIEW_DIR), {
      recursive: true,
    });
    await assertRejects(
      () => createPortableArchive(buildDir, directory, "fixed-webview"),
      Error,
      "WebView2 runtime not found",
    );
    await createPortableArchive(buildDir, directory, "standard");
    assertEquals(
      await Deno.readTextFile(path.join(buildDir, "Clash Nyanpasu.exe")),
      "binary:Clash Nyanpasu.exe",
    );
  });
});
