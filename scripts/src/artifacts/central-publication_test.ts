import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  classifyPublicationArtifactDirectory,
  prepareCentralPublication,
} from "./central-publication.ts";

const commit = "a".repeat(40);
const publishedAt = "2026-10-04T12:34:56.789Z";

async function writeArtifact(
  root: string,
  directory: string,
  relative: string,
  value = "artifact",
) {
  const file = `${root}/${directory}/${relative}`;
  await Deno.mkdir(file.slice(0, file.lastIndexOf("/")), { recursive: true });
  await Deno.writeTextFile(file, value);
}

async function fixture(root: string): Promise<void> {
  const directories = [
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
  for (const directory of directories) {
    const targetName = directory.includes("windows-x86_64")
      ? "x64"
      : directory.includes("windows-aarch64")
      ? "arm64"
      : directory.includes("linux-x86_64")
      ? "amd64"
      : directory.includes("linux-aarch64")
      ? "arm64"
      : directory.includes("macOS-amd64")
      ? "x86_64"
      : "aarch64";
    if (directory.includes("windows") && directory.includes("nsis")) {
      const prefix = directory.includes("fixed-webview")
        ? `Clash.Nyanpasu_fixed-webview_${targetName}`
        : `Clash.Nyanpasu_${targetName}`;
      const updaterPath = `bundle/nsis-updater/${prefix}-setup.exe`;
      await writeArtifact(root, directory, updaterPath);
      await writeArtifact(
        root,
        directory,
        `bundle/nsis-updater/${prefix}.nsis.zip`,
      );
      await writeArtifact(
        root,
        directory,
        `bundle/nsis-updater/${prefix}.nsis.zip.sig`,
      );
    } else if (directory.includes("windows")) {
      await writeArtifact(
        root,
        directory,
        `Clash.Nyanpasu_${
          directory.includes("fixed-webview") ? "fixed-webview_" : ""
        }${targetName}_portable.zip`,
      );
    } else if (directory.includes("linux") && directory.endsWith("appimage")) {
      await writeArtifact(root, directory, "Clash.Nyanpasu.AppImage");
      await writeArtifact(root, directory, "Clash.Nyanpasu.AppImage.tar.gz");
      await writeArtifact(
        root,
        directory,
        "Clash.Nyanpasu.AppImage.tar.gz.sig",
      );
    } else if (directory.includes("linux")) {
      await writeArtifact(
        root,
        directory,
        `Clash.Nyanpasu_${targetName}.${
          directory.endsWith("deb") ? "deb" : "rpm"
        }`,
      );
    } else {
      await writeArtifact(root, directory, `Clash.Nyanpasu_${targetName}.dmg`);
      const archive = `Clash.Nyanpasu_${targetName}.app.tar.gz`;
      await writeArtifact(root, directory, archive);
      await writeArtifact(root, directory, `${archive}.sig`);
    }
  }
}

Deno.test("classify package artifact directories to exactly the updater targets", () => {
  assertEquals(
    classifyPublicationArtifactDirectory(
      "Clash.Nyanpasu-windows-x86_64-nsis-installer",
    ),
    "windows-x86_64",
  );
  assertEquals(
    classifyPublicationArtifactDirectory("Clash.Nyanpasu-linux-aarch64-rpm"),
    "linux-aarch64",
  );
  assertEquals(
    classifyPublicationArtifactDirectory("Clash.Nyanpasu-macOS-amd64"),
    "macos-x86_64",
  );
  assertEquals(
    classifyPublicationArtifactDirectory("unrelated-artifact"),
    null,
  );
});

Deno.test("central preparation validates all targets, normalizes Windows names and shares one timestamp", async () => {
  const temp = await Deno.makeTempDir();
  try {
    const downloaded = `${temp}/downloaded`;
    const output = `${temp}/central`;
    await fixture(downloaded);
    const context = await prepareCentralPublication(downloaded, output, {
      channel: "nightly",
      tag: null,
      commit,
      runId: "77",
      attempt: "2",
      itemPrefix: "mirror-test",
      publishedAt,
    });
    assertEquals(context.publishedAt, publishedAt);
    assertEquals(context.targets.length, 6);
    const windows = JSON.parse(
      await Deno.readTextFile(`${output}/manifests/windows-x86_64.json`),
    );
    assertEquals(windows.publishedAt, publishedAt);
    assertEquals(
      windows.artifacts.map((artifact: { fileName: string }) =>
        artifact.fileName
      )
        .sort(),
      [
        "Clash.Nyanpasu_x64-updater.exe",
        "Clash.Nyanpasu_x64.nsis.zip",
        "Clash.Nyanpasu_x64.nsis.zip.sig",
        "Clash.Nyanpasu_x64_portable.zip",
        "Clash.Nyanpasu_fixed-webview_x64-updater.exe",
        "Clash.Nyanpasu_fixed-webview_x64.nsis.zip",
        "Clash.Nyanpasu_fixed-webview_x64.nsis.zip.sig",
        "Clash.Nyanpasu_fixed-webview_x64_portable.zip",
      ].sort(),
    );
    assertEquals(windows.folderPath, `nightly/77-2-${commit}`);
  } finally {
    await Deno.remove(temp, { recursive: true });
  }
});

Deno.test("central preparation rejects duplicate basenames before output", async () => {
  const temp = await Deno.makeTempDir();
  try {
    const downloaded = `${temp}/downloaded`;
    const output = `${temp}/central`;
    await fixture(downloaded);
    await writeArtifact(
      downloaded,
      "Clash.Nyanpasu-linux-aarch64-deb",
      "Clash.Nyanpasu_amd64.rpm",
    );
    await assertRejects(
      () =>
        prepareCentralPublication(downloaded, output, {
          channel: "release",
          tag: "v1.2.3",
          commit,
          runId: "77",
          attempt: "2",
          itemPrefix: "mirror-test",
          publishedAt,
        }),
      Error,
      "Duplicate publication basename",
    );
    assertEquals(await Deno.stat(output).then(() => true, () => false), false);
  } finally {
    await Deno.remove(temp, { recursive: true });
  }
});

Deno.test("central preparation rejects a missing artifact category before output", async () => {
  const temp = await Deno.makeTempDir();
  try {
    const downloaded = `${temp}/downloaded`;
    const output = `${temp}/central`;
    await fixture(downloaded);
    await Deno.remove(
      `${downloaded}/Clash.Nyanpasu-windows-aarch64-fixed-webview-portable`,
      { recursive: true },
    );
    await assertRejects(
      () =>
        prepareCentralPublication(downloaded, output, {
          channel: "nightly",
          tag: null,
          commit,
          runId: "77",
          attempt: "2",
          itemPrefix: "mirror-test",
          publishedAt,
        }),
      Error,
      "Clash.Nyanpasu-windows-aarch64-fixed-webview-portable",
    );
    assertEquals(await Deno.stat(output).then(() => true, () => false), false);
  } finally {
    await Deno.remove(temp, { recursive: true });
  }
});

Deno.test("central output cannot contain or replace the input directory", async () => {
  const temp = await Deno.makeTempDir();
  try {
    const downloaded = `${temp}/downloaded`;
    await fixture(downloaded);
    await assertRejects(
      () =>
        prepareCentralPublication(downloaded, temp, {
          channel: "nightly",
          tag: null,
          commit,
          runId: "77",
          attempt: "2",
          itemPrefix: "mirror-test",
          publishedAt,
        }),
      Error,
      "must not contain or be contained",
    );
    assertEquals(
      await Deno.stat(downloaded).then(() => true, () => false),
      true,
    );
  } finally {
    await Deno.remove(temp, { recursive: true });
  }
});

Deno.test("central preparation rejects missing updater archives and signature pairs", async () => {
  for (const missingArchive of [false, true]) {
    const temp = await Deno.makeTempDir();
    try {
      const downloaded = `${temp}/downloaded`;
      await fixture(downloaded);
      const archive =
        `${downloaded}/Clash.Nyanpasu-windows-x86_64-fixed-webview-nsis-installer/bundle/nsis-updater/Clash.Nyanpasu_fixed-webview_x64.nsis.zip`;
      await Deno.remove(missingArchive ? archive : `${archive}.sig`);
      await assertRejects(
        () =>
          prepareCentralPublication(downloaded, `${temp}/output`, {
            channel: "nightly",
            tag: null,
            commit,
            runId: "77",
            attempt: "2",
            itemPrefix: "mirror-test",
            publishedAt,
          }),
        Error,
        missingArchive
          ? "Missing required updater archives"
          : "Missing signature files",
      );
      assertEquals(
        await Deno.stat(`${temp}/output`).then(() => true, () => false),
        false,
      );
    } finally {
      await Deno.remove(temp, { recursive: true });
    }
  }
});

Deno.test("central preparation requires the package file from every artifact category", async () => {
  const missingPackages = [
    [
      "Clash.Nyanpasu-windows-x86_64-nsis-installer",
      "bundle/nsis-updater/Clash.Nyanpasu_x64-setup.exe",
    ],
    [
      "Clash.Nyanpasu-windows-aarch64-portable",
      "Clash.Nyanpasu_arm64_portable.zip",
    ],
    [
      "Clash.Nyanpasu-linux-x86_64-appimage",
      "Clash.Nyanpasu.AppImage",
    ],
    ["Clash.Nyanpasu-linux-x86_64-deb", "Clash.Nyanpasu_amd64.deb"],
    ["Clash.Nyanpasu-linux-aarch64-rpm", "Clash.Nyanpasu_arm64.rpm"],
    ["Clash.Nyanpasu-macOS-amd64", "Clash.Nyanpasu_x86_64.dmg"],
  ];

  for (const [directory, packageFile] of missingPackages) {
    const temp = await Deno.makeTempDir();
    try {
      const downloaded = `${temp}/downloaded`;
      await fixture(downloaded);
      await Deno.remove(`${downloaded}/${directory}/${packageFile}`);
      await assertRejects(
        () =>
          prepareCentralPublication(downloaded, `${temp}/output`, {
            channel: "nightly",
            tag: null,
            commit,
            runId: "77",
            attempt: "2",
            itemPrefix: "mirror-test",
            publishedAt,
          }),
        Error,
        `Missing required package artifact in ${directory}`,
      );
      assertEquals(
        await Deno.stat(`${temp}/output`).then(() => true, () => false),
        false,
      );
    } finally {
      await Deno.remove(temp, { recursive: true });
    }
  }
});
