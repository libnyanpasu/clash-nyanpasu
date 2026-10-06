import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import {
  classifyPublicationArtifactDirectory,
  prepareCentralPublication,
} from "./central-publication.ts";
import { recoverCentralPublication } from "./recover-central-publication.ts";

const commit = "a".repeat(40);
const publishedAt = "2026-10-04T12:34:56.789Z";

Deno.test("recovery preserves identities and timestamp across runners and refuses altered bytes", async () => {
  const root = await Deno.makeTempDir();
  const repository = "libnyanpasu/clash-nyanpasu";
  const sourceRun = {
    id: 123,
    run_attempt: 2,
    head_sha: commit,
    status: "completed",
    event: "workflow_dispatch",
    path: ".github/workflows/target-dev-build.yaml",
    repository: { full_name: repository },
    head_repository: { full_name: repository },
  };
  try {
    await fixture(`${root}/downloaded`);
    const original = await prepareCentralPublication(
      `${root}/downloaded`,
      `${root}/original`,
      {
        channel: "nightly",
        tag: null,
        commit,
        runId: "123",
        attempt: "1",
        itemPrefix: "nyanpasu",
        publishedAt,
      },
    );
    const recovered = await recoverCentralPublication(
      sourceRun,
      repository,
      `${root}/downloaded`,
      `${root}/original`,
      `${root}/restored`,
      "nyanpasu",
    );
    assertEquals(recovered.buildId, original.buildId);
    assertEquals(recovered.publishedAt, publishedAt);
    assertEquals(recovered.inventories, original.inventories);
    const manifest = JSON.parse(
      await Deno.readTextFile(recovered.manifests["windows-x86_64"] as string),
    );
    assertEquals(
      manifest.artifacts.every((artifact: { path: string }) =>
        path.normalize(artifact.path).startsWith(
          path.join(root, "restored", path.SEPARATOR),
        )
      ),
      true,
    );
    await writeArtifact(
      `${root}/downloaded`,
      "Clash.Nyanpasu-windows-x86_64-portable",
      "Clash.Nyanpasu_x64_portable.zip",
      "different bytes",
    );
    await assertRejects(
      () =>
        recoverCentralPublication(
          sourceRun,
          repository,
          `${root}/downloaded`,
          `${root}/original`,
          `${root}/altered`,
          "nyanpasu",
        ),
      Error,
      "differs from the original publication",
    );
    await assertRejects(
      () =>
        recoverCentralPublication(
          sourceRun,
          repository,
          `${root}/downloaded`,
          `${root}/original`,
          `${root}/wrong-prefix`,
          "other",
        ),
      Error,
      "differs from the original publication",
    );
    await assertRejects(
      () =>
        recoverCentralPublication(
          { ...sourceRun, head_sha: "b".repeat(40) },
          repository,
          `${root}/downloaded`,
          `${root}/original`,
          `${root}/wrong-sha`,
          "nyanpasu",
        ),
      Error,
      "does not match the source run",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("recovery rejects fork and non-publication runs before reading any files", async () => {
  const repository = "libnyanpasu/clash-nyanpasu";
  const sourceRun = {
    id: 123,
    run_attempt: 1,
    head_sha: commit,
    status: "completed",
    event: "workflow_dispatch",
    path: ".github/workflows/target-dev-build.yaml",
    repository: { full_name: repository },
    head_repository: { full_name: repository },
  };
  for (
    const run of [
      { ...sourceRun, head_repository: { full_name: "fork/clash-nyanpasu" } },
      { ...sourceRun, path: ".github/workflows/ci.yml" },
      { ...sourceRun, status: "in_progress" },
      { ...sourceRun, event: "pull_request" },
    ]
  ) {
    await assertRejects(
      () =>
        recoverCentralPublication(
          run,
          repository,
          "missing",
          "missing",
          "missing",
          "nyanpasu",
        ),
      Error,
      "completed package publication in this repository",
    );
  }
});

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

async function fixture(
  root: string,
  nsisDirectory = "bundle/nsis-updater",
): Promise<void> {
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
      const updaterPath = `${nsisDirectory}/${prefix}-setup.exe`;
      await writeArtifact(root, directory, updaterPath);
      await writeArtifact(
        root,
        directory,
        `${nsisDirectory}/${prefix}.nsis.zip`,
      );
      await writeArtifact(
        root,
        directory,
        `${nsisDirectory}/${prefix}.nsis.zip.sig`,
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

Deno.test("central preparation accepts NSIS setup installers without an updater executable", async () => {
  const temp = await Deno.makeTempDir();
  try {
    const downloaded = `${temp}/downloaded`;
    const output = `${temp}/central`;
    await fixture(downloaded, "bundle/nsis");
    await prepareCentralPublication(downloaded, output, {
      channel: "nightly",
      tag: null,
      commit,
      runId: "77",
      attempt: "2",
      itemPrefix: "mirror-test",
      publishedAt,
    });
    for (const target of ["windows-x86_64", "windows-aarch64"]) {
      const manifest = JSON.parse(
        await Deno.readTextFile(`${output}/manifests/${target}.json`),
      );
      const names = manifest.artifacts.map((artifact: { fileName: string }) =>
        artifact.fileName
      );
      assertEquals(
        names.filter((name: string) => name.endsWith("-setup.exe")).length,
        2,
      );
      assertEquals(
        names.some((name: string) => name.endsWith("-updater.exe")),
        false,
      );
    }
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
      "bundle/nsis/Clash.Nyanpasu_x64-setup.exe",
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
      await fixture(downloaded, "bundle/nsis");
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
