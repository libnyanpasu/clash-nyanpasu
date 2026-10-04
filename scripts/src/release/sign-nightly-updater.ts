import { createRequire } from "node:module";
import * as path from "jsr:@std/path";
import { globby } from "npm:globby";

async function main() {
  const workspaceRoot = Deno.cwd();
  const { version } = JSON.parse(
    await Deno.readTextFile(path.join(workspaceRoot, "package.json")),
  );
  const archives = await globby([
    "backend/target/**/bundle/**/*.nsis.zip",
    "backend/target/**/bundle/**/*.AppImage.tar.gz",
  ], { cwd: workspaceRoot, absolute: true });
  if (archives.length === 0) {
    throw new Error("No nightly updater archives found");
  }

  // Installer versions differ from the release version for Windows and RPM/DEB.
  // Bind updater signatures to the version compiled as NYANPASU_VERSION instead.
  const tauriCli = createRequire(import.meta.url).resolve(
    "@tauri-apps/cli/tauri.js",
  );
  for (const archive of archives) {
    const result = await new Deno.Command("node", {
      args: [tauriCli, "signer", "sign", "--app-version", version, archive],
      cwd: workspaceRoot,
      stdout: "null",
      stderr: "inherit",
    }).output();
    if (!result.success) throw new Error(`Failed to sign ${archive}`);
  }
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(err);
    Deno.exit(1);
  });
}
