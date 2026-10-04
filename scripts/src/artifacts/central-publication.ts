import * as path from "jsr:@std/path";
import {
  createIaItemIdentifier,
  preparePublicationManifest,
  type PublishChannel,
} from "./publication-manifest.ts";
import { validateSourceforgeFilename } from "./sourceforge.ts";

export const CENTRAL_PUBLICATION_TARGETS = [
  "windows-x86_64",
  "windows-aarch64",
  "linux-x86_64",
  "linux-aarch64",
  "macos-x86_64",
  "macos-aarch64",
] as const;

export interface CentralPublicationOptions {
  channel: PublishChannel;
  tag: string | null;
  commit: string;
  runId: string;
  attempt: string;
  itemPrefix: string;
  publishedAt?: string;
}

export function classifyPublicationArtifactDirectory(
  name: string,
): string | null {
  const windows = /^Clash\.Nyanpasu-windows-(x86_64|aarch64)-/.exec(name);
  if (windows) return `windows-${windows[1]}`;
  const linux = /^Clash\.Nyanpasu-linux-(x86_64|aarch64)-/.exec(name);
  if (linux) return `linux-${linux[1]}`;
  const macos = /^Clash\.Nyanpasu-macOS-(amd64|x86_64|aarch64)$/.exec(name);
  if (macos) return `macos-${macos[1] === "amd64" ? "x86_64" : macos[1]}`;
  if (name.startsWith("Clash.Nyanpasu-")) {
    throw new Error(`Unrecognized package artifact directory: ${name}`);
  }
  return null;
}

async function filesUnder(root: string): Promise<string[]> {
  const files: string[] = [];
  for await (const entry of Deno.readDir(root)) {
    const fullPath = path.join(root, entry.name);
    if (entry.isSymlink) {
      throw new Error(`Symlink in downloaded build artifacts: ${fullPath}`);
    }
    if (entry.isDirectory) files.push(...await filesUnder(fullPath));
    else if (entry.isFile) files.push(fullPath);
  }
  return files;
}

export async function prepareCentralPublication(
  artifactsRoot: string,
  outputRoot: string,
  options: CentralPublicationOptions,
) {
  if (!/^\d+$/.test(options.runId) || !/^\d+$/.test(options.attempt)) {
    throw new Error("Invalid GitHub workflow run identity");
  }
  if (!/^[a-f0-9]{40}$/.test(options.commit)) {
    throw new Error("Commit must be a full 40-character SHA");
  }
  if (
    options.channel === "release" &&
    (!options.tag || !/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,127}$/.test(options.tag))
  ) {
    throw new Error("A safe release tag is required for central publication");
  }
  if (options.channel === "nightly" && options.tag !== null) {
    throw new Error("Nightly central publication cannot include a release tag");
  }
  const publishedAt = options.publishedAt ?? new Date().toISOString();
  if (new Date(publishedAt).toISOString() !== publishedAt) {
    throw new Error("publishedAt must be a canonical UTC ISO timestamp");
  }

  const inputRoot = path.resolve(artifactsRoot);
  const output = path.resolve(outputRoot);
  const byTarget = new Map<string, Array<{ path: string; fileName: string }>>(
    CENTRAL_PUBLICATION_TARGETS.map((target) => [target, []]),
  );
  const artifactDirectories = new Set<string>();
  const names = new Map<string, string>();
  for await (const entry of Deno.readDir(inputRoot)) {
    if (entry.isSymlink) {
      throw new Error(`Symlink in downloaded artifact root: ${entry.name}`);
    }
    if (!entry.isDirectory) continue;
    const target = classifyPublicationArtifactDirectory(entry.name);
    if (!target) continue;
    artifactDirectories.add(entry.name);
    const artifactDir = path.join(inputRoot, entry.name);
    const directoryFiles: string[] = [];
    for (const sourcePath of await filesUnder(artifactDir)) {
      const relative = path.relative(artifactDir, sourcePath);
      const segments = relative.split(path.SEPARATOR);
      const originalName = path.basename(sourcePath);
      const fileName =
        segments.includes("nsis-updater") && originalName.endsWith("-setup.exe")
          ? `${originalName.slice(0, -"-setup.exe".length)}-updater.exe`
          : originalName;
      validateSourceforgeFilename(fileName);
      const duplicate = names.get(fileName);
      if (duplicate) {
        throw new Error(
          `Duplicate publication basename ${fileName}: ${duplicate} and ${sourcePath}`,
        );
      }
      names.set(fileName, sourcePath);
      byTarget.get(target)!.push({ path: sourcePath, fileName });
      directoryFiles.push(fileName);
    }
    const requiredPackageExtensions = entry.name.includes("windows")
      ? entry.name.includes("nsis-installer")
        ? ["-setup.exe", "-updater.exe"]
        : ["_portable.zip"]
      : entry.name.endsWith("-appimage")
      ? [".AppImage"]
      : entry.name.endsWith("-deb")
      ? [".deb"]
      : entry.name.endsWith("-rpm")
      ? [".rpm"]
      : [".dmg"];
    if (
      !directoryFiles.some((fileName) =>
        requiredPackageExtensions.some((extension) =>
          fileName.endsWith(extension)
        )
      )
    ) {
      throw new Error(
        `Missing required package artifact in ${entry.name} (expected ${
          requiredPackageExtensions.join(" or ")
        })`,
      );
    }
  }

  const emptyTargets = CENTRAL_PUBLICATION_TARGETS.filter((target) =>
    byTarget.get(target)!.length === 0
  );
  if (emptyTargets.length) {
    throw new Error(
      `Missing finalized package artifacts for: ${emptyTargets.join(", ")}`,
    );
  }
  const expectedDirectories = [
    "Clash.Nyanpasu-windows-x86_64-nsis-installer",
    "Clash.Nyanpasu-windows-x86_64-portable",
    "Clash.Nyanpasu-windows-x86_64-fixed-webview-nsis-installer",
    "Clash.Nyanpasu-windows-x86_64-fixed-webview-portable",
    "Clash.Nyanpasu-windows-aarch64-nsis-installer",
    "Clash.Nyanpasu-windows-aarch64-portable",
    "Clash.Nyanpasu-windows-aarch64-fixed-webview-nsis-installer",
    "Clash.Nyanpasu-windows-aarch64-fixed-webview-portable",
    "Clash.Nyanpasu-linux-x86_64-appimage",
    "Clash.Nyanpasu-linux-x86_64-deb",
    "Clash.Nyanpasu-linux-x86_64-rpm",
    "Clash.Nyanpasu-linux-aarch64-deb",
    "Clash.Nyanpasu-linux-aarch64-rpm",
    "Clash.Nyanpasu-macOS-amd64",
    "Clash.Nyanpasu-macOS-aarch64",
  ];
  const missingDirectories = expectedDirectories.filter((name) =>
    !artifactDirectories.has(name)
  );
  if (missingDirectories.length) {
    throw new Error(
      `Missing required build artifact directories: ${
        missingDirectories.join("; ")
      }`,
    );
  }
  for (const target of CENTRAL_PUBLICATION_TARGETS) {
    const files = byTarget.get(target)!;
    const extension = target.startsWith("windows-")
      ? ".nsis.zip"
      : target.startsWith("macos-")
      ? ".app.tar.gz"
      : target === "linux-x86_64"
      ? ".AppImage.tar.gz"
      : null;
    if (!extension) continue;
    const requiredCount = target.startsWith("windows-") ? 2 : 1;
    const archives = files.filter(({ fileName }) =>
      fileName.endsWith(extension)
    );
    if (
      archives.length !== requiredCount ||
      (target.startsWith("windows-") &&
        archives.filter(({ fileName }) => fileName.includes("fixed-webview"))
            .length !== 1)
    ) {
      throw new Error(`Missing required updater archives for ${target}`);
    }
  }
  const allFiles = [...byTarget.values()].flat();
  const publishedNames = new Set(allFiles.map((artifact) => artifact.fileName));
  const signedArchives = allFiles.filter(({ fileName }) =>
    fileName.endsWith(".nsis.zip") || fileName.endsWith(".AppImage.tar.gz") ||
    fileName.endsWith(".app.tar.gz")
  );
  const unsignedArchives = signedArchives.filter(({ fileName }) =>
    !publishedNames.has(`${fileName}.sig`)
  );
  if (unsignedArchives.length) {
    throw new Error(
      `Missing signature files for updater archives: ${
        unsignedArchives.map(({ fileName }) => fileName).join(", ")
      }`,
    );
  }
  if (names.size === 0) {
    throw new Error("No finalized package artifacts were downloaded");
  }

  const globalBuildId = options.channel === "nightly"
    ? `${options.runId}-${options.attempt}-${options.commit}`
    : options.tag!;
  const folderPath = options.channel === "nightly"
    ? `nightly/${options.runId}-${options.attempt}-${options.commit}`
    : `release/${options.tag}`;
  const isWithin = (parent: string, candidate: string) => {
    const relative = path.relative(parent, candidate);
    return relative === "" ||
      (!path.isAbsolute(relative) && relative !== ".." &&
        !relative.startsWith(`..${path.SEPARATOR}`));
  };
  if (isWithin(inputRoot, output) || isWithin(output, inputRoot)) {
    throw new Error(
      "Publication output must not contain or be contained by the downloaded artifact root",
    );
  }
  await Deno.remove(output, { recursive: true }).catch((error) => {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  });
  await Deno.mkdir(path.join(output, "payloads"), { recursive: true });
  await Deno.mkdir(path.join(output, "manifests"), { recursive: true });

  const manifests: Record<string, unknown> = {};
  const inventories: Record<string, string[]> = {};
  for (const target of CENTRAL_PUBLICATION_TARGETS) {
    const targetDir = path.join(output, "payloads", target);
    await Deno.mkdir(targetDir, { recursive: true });
    const files: string[] = [];
    for (
      const artifact of byTarget.get(target)!.sort((a, b) =>
        a.fileName.localeCompare(b.fileName)
      )
    ) {
      const destination = path.join(targetDir, artifact.fileName);
      await Deno.copyFile(artifact.path, destination);
      files.push(destination);
    }
    const buildId =
      `${options.runId}-${options.attempt}-${options.commit}-${target}`;
    const itemIdentifier = createIaItemIdentifier(
      options.itemPrefix || "sourceforge-only",
      options.channel,
      options.runId,
      options.attempt,
      target,
      options.commit,
    );
    const manifest = await preparePublicationManifest({
      channel: options.channel,
      target,
      buildId,
      itemPrefix: options.itemPrefix,
      commit: options.commit,
      tag: options.tag,
      folderPath,
      itemIdentifier,
      publishedAt,
      paths: files,
    });
    const manifestPath = path.join(output, "manifests", `${target}.json`);
    await Deno.writeTextFile(
      manifestPath,
      `${JSON.stringify(manifest, null, 2)}\n`,
    );
    manifests[target] = manifestPath;
    inventories[target] = manifest.artifacts.map((artifact) =>
      artifact.fileName
    );
  }
  const context = {
    schemaVersion: 1,
    channel: options.channel,
    buildId: globalBuildId,
    folderPath,
    commit: options.commit,
    tag: options.tag,
    publishedAt,
    targets: CENTRAL_PUBLICATION_TARGETS,
    manifests,
    inventories,
  };
  await Deno.writeTextFile(
    path.join(output, "publication-context.json"),
    `${JSON.stringify(context, null, 2)}\n`,
  );
  return context;
}
