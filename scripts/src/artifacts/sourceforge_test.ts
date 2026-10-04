import { assertEquals, assertRejects, assertThrows } from "jsr:@std/assert@1";
import {
  guardReleaseMirrorUpload,
  reserveSourceforgeArtifact,
  sourceforgeDownloadUrl,
  sourceforgeUploadBatch,
  sourceforgeWebUploadBatch,
  validateSourceforgeFilename,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from "./sourceforge.ts";

Deno.test("SourceForge inputs constrain project, account and SFTP filenames", () => {
  validateSourceforgeProject("nyanpasu-project");
  validateSourceforgeUsername("build.bot");
  validateSourceforgeFilename("Clash.Nyanpasu-x64-setup.exe");
  assertThrows(() => validateSourceforgeProject("bad-project-"), Error);
  assertThrows(() => validateSourceforgeProject("a".repeat(64)), Error);
  assertThrows(() => validateSourceforgeUsername("-option-like"), Error);
  assertThrows(() => validateSourceforgeFilename("file[1].zip"), Error);
});

Deno.test("canonical binary URL uses immutable FRS path and encodes a basename", () => {
  assertEquals(
    sourceforgeDownloadUrl(
      "nyanpasu",
      "nightly/123-1-" + "a".repeat(40),
      "Clash Nyanpasu.zip",
    ),
    `https://downloads.sourceforge.net/project/nyanpasu/nightly/123-1-${
      "a".repeat(40)
    }/Clash%20Nyanpasu.zip`,
  );
  assertThrows(
    () => sourceforgeDownloadUrl("nyanpasu", "latest", "thing.zip"),
    Error,
  );
});

Deno.test("binary upload batch contains only explicit managed filenames", () => {
  const batch = sourceforgeUploadBatch(
    "nyanpasu",
    "releases/v1.2.3",
    [{ path: "/tmp/one.zip", fileName: "one.zip" }],
    false,
  );
  assertEquals(batch.includes('put "/tmp/one.zip"'), true);
  assertThrows(() =>
    sourceforgeUploadBatch(
      "nyanpasu",
      "nightly/latest",
      [{ path: "/tmp/a", fileName: "a" }],
      false,
    ), Error);
  assertThrows(() =>
    sourceforgeUploadBatch(
      "nyanpasu",
      "nightly/123",
      [{ path: "/tmp/a", fileName: "a[1]" }],
      false,
    ), Error);
});

Deno.test("web manifest batch stages every selected feed before promoting any", () => {
  const batch = sourceforgeWebUploadBatch("nyanpasu", [
    {
      path: "/tmp/update.json",
      fileName: "update.json",
      stagedName: "stage-update.json",
    },
    {
      path: "/tmp/update-nightly.json",
      fileName: "update-nightly.json",
      stagedName: "stage-update-nightly.json",
    },
  ]);
  const lastPut = batch.lastIndexOf("put ");
  const firstRename = batch.indexOf("rename ");
  assertEquals(lastPut < firstRename, true);
  assertEquals(batch.split("rename ").length - 1, 2);
});

const releaseArtifacts = [
  {
    fileName: "Clash.Nyanpasu_x64.nsis.zip",
    fileSize: 12,
    sha256: "a".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x64.nsis.zip.sig",
    fileSize: 8,
    sha256: "b".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x64-updater.exe",
    fileSize: 10,
    sha256: "c".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_fixed-webview-x64.nsis.zip",
    fileSize: 14,
    sha256: "d".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_fixed-webview-x64.nsis.zip.sig",
    fileSize: 9,
    sha256: "e".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_fixed-webview-x64-updater.exe",
    fileSize: 11,
    sha256: "f".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_fixed-webview_x64_portable.zip",
    fileSize: 13,
    sha256: "1".repeat(64),
  },
];

const fullReleaseArtifacts = [
  ...releaseArtifacts,
  {
    fileName: "Clash.Nyanpasu_amd64.deb",
    fileSize: 15,
    sha256: "2".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x86_64.rpm",
    fileSize: 16,
    sha256: "3".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x86_64.rpm.sha256",
    fileSize: 17,
    sha256: "4".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_arm64.aarch64.rpm",
    fileSize: 18,
    sha256: "5".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x86_64.app.tar.gz",
    fileSize: 19,
    sha256: "6".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_x86_64.dmg",
    fileSize: 20,
    sha256: "7".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_aarch64.app.tar.gz",
    fileSize: 21,
    sha256: "8".repeat(64),
  },
  {
    fileName: "Clash.Nyanpasu_aarch64.dmg",
    fileSize: 22,
    sha256: "9".repeat(64),
  },
];

function priorReleaseMirror(artifacts = releaseArtifacts) {
  const project = "nyanpasu";
  const releaseTag = "v1.2.3";
  return {
    schemaVersion: 1,
    project,
    channel: "release",
    buildId: releaseTag,
    remotePath: `releases/${releaseTag}`,
    assets: Object.fromEntries(artifacts.map((artifact) => [
      artifact.fileName,
      {
        fileSize: artifact.fileSize,
        sha256: artifact.sha256,
        url: sourceforgeDownloadUrl(
          project,
          `releases/${releaseTag}`,
          artifact.fileName,
        ),
      },
    ])),
  };
}

Deno.test("release retry allows the same recorded target inventory", async () => {
  let uploads = 0;
  await guardReleaseMirrorUpload(
    "nyanpasu",
    "v1.2.3",
    { target: "windows-x86_64", artifacts: releaseArtifacts },
    async () => priorReleaseMirror(),
    async () => {
      uploads++;
    },
    async () => {},
  );
  assertEquals(uploads, 1);
});

Deno.test("release retry rejects changed or extra bytes before SFTP upload", async () => {
  for (
    const artifacts of [
      releaseArtifacts.map((artifact, index) =>
        index === 0 ? { ...artifact, sha256: "d".repeat(64) } : artifact
      ),
      [...releaseArtifacts, {
        fileName: "Clash.Nyanpasu_x64-extra.zip",
        fileSize: 1,
        sha256: "e".repeat(64),
      }],
    ]
  ) {
    let uploads = 0;
    await assertRejects(
      () =>
        guardReleaseMirrorUpload(
          "nyanpasu",
          "v1.2.3",
          { target: "windows-x86_64", artifacts },
          async () => priorReleaseMirror(),
          async () => {
            uploads++;
          },
          async () => {},
        ),
      Error,
      "immutable",
    );
    assertEquals(uploads, 0);
  }
});

Deno.test("release retry can repair a partial prior upload without replacing omitted bytes", async () => {
  let uploads = 0;
  await guardReleaseMirrorUpload(
    "nyanpasu",
    "v1.2.3",
    {
      target: "windows-x86_64",
      artifacts: releaseArtifacts.slice(0, -1),
    },
    async () => priorReleaseMirror(),
    async () => {
      uploads++;
    },
    async () => {},
  );
  assertEquals(uploads, 1);
});

Deno.test("release upload proceeds without a sidecar but fails closed on lookup errors", async () => {
  let uploads = 0;
  const candidate = { target: "windows-x86_64", artifacts: releaseArtifacts };
  await guardReleaseMirrorUpload(
    "nyanpasu",
    "v1.2.3",
    candidate,
    async () => null,
    async () => {
      uploads++;
    },
    async () => {},
  );
  assertEquals(uploads, 1);

  await assertRejects(
    () =>
      guardReleaseMirrorUpload(
        "nyanpasu",
        "v1.2.3",
        candidate,
        async () => {
          throw new Error("GitHub API unavailable");
        },
        async () => {
          uploads++;
        },
        async () => {},
      ),
    Error,
    "GitHub API unavailable",
  );
  assertEquals(uploads, 1);
});

Deno.test("release backfill retries accept the exact complete cross-platform inventory", async () => {
  let uploads = 0;
  const prior = priorReleaseMirror(fullReleaseArtifacts);
  await guardReleaseMirrorUpload(
    "nyanpasu",
    "v1.2.3",
    { target: "release-backfill", artifacts: fullReleaseArtifacts },
    async () => prior,
    async () => {
      uploads++;
    },
    async () => {},
  );
  assertEquals(uploads, 1);

  await assertRejects(
    () =>
      guardReleaseMirrorUpload(
        "nyanpasu",
        "v1.2.3",
        {
          target: "release-backfill",
          artifacts: fullReleaseArtifacts.slice(0, -1),
        },
        async () => prior,
        async () => {
          uploads++;
        },
        async () => {},
      ),
    Error,
    "immutable",
  );
  assertEquals(uploads, 1);
});

Deno.test("release reservations protect partial uploads before a GitHub sidecar exists", async () => {
  let inventory: unknown;
  let uploads = 0;
  const publish = (
    artifact: typeof releaseArtifacts[number],
    failUpload = false,
  ) =>
    guardReleaseMirrorUpload(
      "nyanpasu",
      "v1.2.3",
      { target: "windows-x86_64", artifacts: [artifact] },
      async () => null,
      async () => {
        uploads++;
        if (failUpload) {
          throw new Error("SFTP interrupted after partial upload");
        }
      },
      () =>
        reserveSourceforgeArtifact(
          artifact,
          async () => {
            if (inventory !== undefined) {
              throw new Error("Claim already exists");
            }
            inventory = { ...artifact };
          },
          async () => inventory,
        ),
    );
  await assertRejects(
    () => publish(releaseArtifacts[0], true),
    Error,
    "interrupted",
  );
  await assertRejects(
    () => publish({ ...releaseArtifacts[0], sha256: "f".repeat(64) }),
    Error,
    "immutable",
  );
  assertEquals(uploads, 1);
  await publish(releaseArtifacts[0]);
  assertEquals(uploads, 2);
});

Deno.test("incomplete or unreadable release reservations fail before uploading", async () => {
  for (
    const inventory of [null, {}, { ...releaseArtifacts[0], fileSize: 99 }]
  ) {
    await assertRejects(() =>
      reserveSourceforgeArtifact(
        releaseArtifacts[0],
        async () => {
          throw new Error("Claim already exists");
        },
        async () => inventory,
      )
    );
  }
  await assertRejects(
    () =>
      reserveSourceforgeArtifact(
        releaseArtifacts[0],
        async () => {
          throw new Error("Connection lost during claim creation");
        },
        async () => {
          throw new Error("Inventory unavailable");
        },
      ),
    Error,
    "Inventory unavailable",
  );
});
