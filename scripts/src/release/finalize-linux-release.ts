import * as path from "jsr:@std/path";
import { globby } from "npm:globby";
import { linuxPackageVersions } from "./prepare-release.ts";
import { consola } from "../shared/logger.ts";

// Release builds bundle Linux packages as `X.Y.Z` (see prepare-release.ts).
// This sets the deb Version to its prerelease form and puts the release
// version back into the deb and AppImage file names. Run it after the updater
// archive has been re-signed and before checksums are computed.

const decoder = new TextDecoder();

async function run(command: string, args: string[], cwd?: string) {
  const output = await new Deno.Command(command, {
    args,
    cwd,
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!output.success) {
    throw new Error(
      `${command} ${args.join(" ")} failed: ${decoder.decode(output.stderr)}`,
    );
  }
  return decoder.decode(output.stdout);
}

export function setControlVersion(control: string, version: string) {
  if (!/^Version: .*$/m.test(control)) {
    throw new Error("control file has no Version field");
  }
  return control.replace(/^Version: .*$/m, `Version: ${version}`);
}

export function releaseFileName(
  name: string,
  tauriVersion: string,
  version: string,
) {
  const bundled = `_${tauriVersion}_`;
  if (!name.includes(bundled)) {
    throw new Error(`${name} does not carry the bundled version`);
  }
  return name.replace(bundled, `_${version}_`);
}

function tarCompression(member: string) {
  if (member.endsWith(".gz")) return ["-z"];
  if (member.endsWith(".xz")) return ["-J"];
  if (member.endsWith(".zst")) return ["--zstd"];
  if (member.endsWith(".tar")) return [];
  throw new Error(`unsupported deb control member ${member}`);
}

export async function debVersion(deb: string) {
  return (await run("dpkg-deb", ["--field", deb, "Version"])).trim();
}

/** Replaces only the control member, so `data.tar` and its md5sums stay intact. */
export async function setDebVersion(deb: string, version: string) {
  deb = path.resolve(deb);
  const control = (await run("ar", ["t", deb]))
    .split("\n")
    .find((member) => member.startsWith("control.tar"));
  if (!control) throw new Error(`${deb} has no control member`);

  const work = await Deno.makeTempDir({ prefix: "nyanpasu-deb-" });
  try {
    const dir = path.join(work, "control");
    await Deno.mkdir(dir);
    await run("ar", ["x", deb, control], work);
    await run("tar", ["-xf", control, "-C", dir], work);
    const controlFile = path.join(dir, "control");
    await Deno.writeTextFile(
      controlFile,
      setControlVersion(await Deno.readTextFile(controlFile), version),
    );
    await Deno.remove(path.join(work, control));
    await run("tar", [
      "-c",
      ...tarCompression(control),
      "-f",
      control,
      "--owner=0",
      "--group=0",
      "--numeric-owner",
      "-C",
      dir,
      ".",
    ], work);
    await run("ar", ["r", deb, control], work);
  } finally {
    await Deno.remove(work, { recursive: true });
  }

  const written = await debVersion(deb);
  if (written !== version) {
    throw new Error(`${deb} has Version ${written}, expected ${version}`);
  }
}

async function main() {
  const root = Deno.cwd();
  const { version } = JSON.parse(
    await Deno.readTextFile(path.join(root, "package.json")),
  );
  const { tauriVersion, debVersion: expected } = linuxPackageVersions(version);

  const debs = await globby("backend/target/**/bundle/deb/*.deb", {
    cwd: root,
    absolute: true,
  });
  if (debs.length === 0) throw new Error("No deb packages found");
  for (const deb of debs) {
    if ((await debVersion(deb)) !== expected) {
      await setDebVersion(deb, expected);
      consola.success(`Set ${path.basename(deb)} Version to ${expected}`);
    }
  }

  if (version === tauriVersion) return;
  const bundles = await globby([
    "backend/target/**/bundle/deb/*.deb",
    "backend/target/**/bundle/appimage/*.AppImage",
    "backend/target/**/bundle/appimage/*.AppImage.tar.gz",
    "backend/target/**/bundle/appimage/*.AppImage.tar.gz.sig",
  ], { cwd: root, absolute: true });
  for (const file of bundles) {
    const renamed = path.join(
      path.dirname(file),
      releaseFileName(path.basename(file), tauriVersion, version),
    );
    await Deno.rename(file, renamed);
    consola.success(
      `Renamed ${path.basename(file)} to ${path.basename(renamed)}`,
    );
  }
}

if (import.meta.main) {
  main().catch((err) => {
    consola.error(err);
    Deno.exit(1);
  });
}
