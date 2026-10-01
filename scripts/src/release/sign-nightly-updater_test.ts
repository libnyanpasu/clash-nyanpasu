import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import {
  collectUpdaterPlatforms,
  type ReleaseAsset,
} from "../updater/updater-platforms.ts";

const tauriCli = path.fromFileUrl(
  new URL(
    "../../../node_modules/.pnpm/@tauri-apps+cli@2.12.0/node_modules/@tauri-apps/cli/tauri.js",
    import.meta.url,
  ),
);

Deno.test("nightly updater signatures bind all installer variants to the release version", async () => {
  const directory = await Deno.makeTempDir({ prefix: "nyanpasu-sign-test-" });
  try {
    const keyPath = path.join(directory, "test.key");
    const generated = await new Deno.Command("node", {
      args: [
        tauriCli,
        "signer",
        "generate",
        "--ci",
        "--password",
        "",
        "--write-keys",
        keyPath,
      ],
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(generated.success, true);
    const env = {
      TAURI_SIGNING_PRIVATE_KEY: await Deno.readTextFile(keyPath),
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
    };
    const version = "2.0.0-alpha+83fbe50";
    await Deno.writeTextFile(
      path.join(directory, "package.json"),
      JSON.stringify({ version }),
    );
    const bundleDir = path.join(directory, "backend/target/release/bundle");
    await Deno.mkdir(bundleDir, { recursive: true });
    const archives = [
      "Clash.Nyanpasu_2.0.0_x64-setup.nsis.zip",
      "Clash.Nyanpasu_2.0.0_fixed-webview-x64-setup.nsis.zip",
      "Clash.Nyanpasu_2.0.0_arm64-setup.nsis.zip",
      "Clash.Nyanpasu_2.0.0_fixed-webview-arm64-setup.nsis.zip",
      "Clash.Nyanpasu_2.0.0+alpha.83fbe50_amd64.AppImage.tar.gz",
    ];
    const assets: ReleaseAsset[] = [];
    for (const name of archives) {
      const archive = path.join(bundleDir, name);
      await Deno.writeTextFile(archive, `updater fixture ${name}`);
      const signed = await new Deno.Command("node", {
        args: [tauriCli, "signer", "sign", "--app-version", "2.0.0", archive],
        env,
        stdout: "null",
        stderr: "piped",
      }).output();
      assertEquals(signed.success, true);
      assets.push(
        { name, browser_download_url: archive },
        { name: `${name}.sig`, browser_download_url: `${archive}.sig` },
      );
    }
    await assertRejects(
      () => collectUpdaterPlatforms(assets, Deno.readTextFile, version),
      Error,
      "Signed version mismatch",
    );
    const result = await new Deno.Command(Deno.execPath(), {
      args: [
        "run",
        "-A",
        path.fromFileUrl(new URL("sign-nightly-updater.ts", import.meta.url)),
      ],
      cwd: directory,
      env,
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(result.success, true, new TextDecoder().decode(result.stderr));
    const platforms = await collectUpdaterPlatforms(
      assets,
      Deno.readTextFile,
      `v${version}`,
    );
    assertEquals(Object.keys(platforms).length, 7);
    for (const { signature } of Object.values(platforms)) {
      assertEquals(
        atob(signature).split("\n")[2].endsWith(`\tversion:${version}`),
        true,
      );
    }
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});
