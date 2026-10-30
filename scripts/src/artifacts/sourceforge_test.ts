import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  guardReleaseMirrorUpload,
  reserveSourceforgeArtifact,
  sourceforgeDownloadUrl,
  sourceforgeReservationBatch,
  sourceforgeUploadBatch,
  sourceforgeWebUploadBatch,
  validateSourceforgeFilename,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from './sourceforge.ts'

test('SourceForge inputs constrain project, account and SFTP filenames', () => {
  validateSourceforgeProject('nyanpasu-project')
  validateSourceforgeUsername('build.bot')
  validateSourceforgeFilename('Clash.Nyanpasu-x64-setup.exe')
  assert.throws(() => validateSourceforgeProject('bad-project-'), Error)
  assert.throws(() => validateSourceforgeProject('a'.repeat(64)), Error)
  assert.throws(() => validateSourceforgeUsername('-option-like'), Error)
  assert.throws(() => validateSourceforgeFilename('file[1].zip'), Error)
})

test('canonical binary URL uses immutable FRS path and encodes a basename', () => {
  assert.deepStrictEqual(
    sourceforgeDownloadUrl(
      'nyanpasu',
      'nightly/123-1-' + 'a'.repeat(40),
      'Clash Nyanpasu.zip',
    ),
    `https://downloads.sourceforge.net/project/nyanpasu/nightly/123-1-${'a'.repeat(
      40,
    )}/Clash%20Nyanpasu.zip`,
  )
  assert.throws(
    () => sourceforgeDownloadUrl('nyanpasu', 'latest', 'thing.zip'),
    Error,
  )
})

test('binary upload batch contains only explicit managed filenames', () => {
  const batch = sourceforgeUploadBatch(
    'nyanpasu',
    'releases/v1.2.3',
    [{ path: '/tmp/one.zip', fileName: 'one.zip' }],
    false,
  )
  assert.deepStrictEqual(batch.includes('put "/tmp/one.zip"'), true)
  assert.throws(
    () =>
      sourceforgeUploadBatch(
        'nyanpasu',
        'nightly/latest',
        [{ path: '/tmp/a', fileName: 'a' }],
        false,
      ),
    Error,
  )
  assert.throws(
    () =>
      sourceforgeUploadBatch(
        'nyanpasu',
        'nightly/123',
        [{ path: '/tmp/a', fileName: 'a[1]' }],
        false,
      ),
    Error,
  )
})

test('web manifest batch stages every selected feed before promoting any', () => {
  const batch = sourceforgeWebUploadBatch('nyanpasu', [
    {
      path: '/tmp/update.json',
      fileName: 'update.json',
      stagedName: 'stage-update.json',
    },
    {
      path: '/tmp/update-nightly.json',
      fileName: 'update-nightly.json',
      stagedName: 'stage-update-nightly.json',
    },
  ])
  const lastPut = batch.lastIndexOf('put ')
  const firstRename = batch.indexOf('rename ')
  assert.deepStrictEqual(lastPut < firstRename, true)
  assert.deepStrictEqual(batch.split('rename ').length - 1, 2)
})

const releaseArtifacts = [
  {
    fileName: 'Clash.Nyanpasu_x64.nsis.zip',
    fileSize: 12,
    sha256: 'a'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x64.nsis.zip.sig',
    fileSize: 8,
    sha256: 'b'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x64-updater.exe',
    fileSize: 10,
    sha256: 'c'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_fixed-webview-x64.nsis.zip',
    fileSize: 14,
    sha256: 'd'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_fixed-webview-x64.nsis.zip.sig',
    fileSize: 9,
    sha256: 'e'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_fixed-webview-x64-updater.exe',
    fileSize: 11,
    sha256: 'f'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_fixed-webview_x64_portable.zip',
    fileSize: 13,
    sha256: '1'.repeat(64),
  },
]

const fullReleaseArtifacts = [
  ...releaseArtifacts,
  {
    fileName: 'Clash.Nyanpasu_amd64.deb',
    fileSize: 15,
    sha256: '2'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x86_64.rpm',
    fileSize: 16,
    sha256: '3'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x86_64.rpm.sha256',
    fileSize: 17,
    sha256: '4'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_arm64.aarch64.rpm',
    fileSize: 18,
    sha256: '5'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x86_64.app.tar.gz',
    fileSize: 19,
    sha256: '6'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_x86_64.dmg',
    fileSize: 20,
    sha256: '7'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_aarch64.app.tar.gz',
    fileSize: 21,
    sha256: '8'.repeat(64),
  },
  {
    fileName: 'Clash.Nyanpasu_aarch64.dmg',
    fileSize: 22,
    sha256: '9'.repeat(64),
  },
]

function priorReleaseMirror(artifacts = releaseArtifacts) {
  const project = 'nyanpasu'
  const releaseTag = 'v1.2.3'
  return {
    schemaVersion: 1,
    project,
    channel: 'release',
    buildId: releaseTag,
    remotePath: `releases/${releaseTag}`,
    assets: Object.fromEntries(
      artifacts.map((artifact) => [
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
      ]),
    ),
  }
}

test('release retry allows the same recorded target inventory', async () => {
  let uploads = 0
  await guardReleaseMirrorUpload(
    'nyanpasu',
    'v1.2.3',
    { target: 'windows-x86_64', artifacts: releaseArtifacts },
    async () => priorReleaseMirror(),
    async () => {
      uploads++
    },
    async () => {},
  )
  assert.deepStrictEqual(uploads, 1)
})

test('release retry rejects changed or extra bytes before SFTP upload', async () => {
  for (const artifacts of [
    releaseArtifacts.map((artifact, index) =>
      index === 0 ? { ...artifact, sha256: 'd'.repeat(64) } : artifact,
    ),
    [
      ...releaseArtifacts,
      {
        fileName: 'Clash.Nyanpasu_x64-extra.zip',
        fileSize: 1,
        sha256: 'e'.repeat(64),
      },
    ],
  ]) {
    let uploads = 0
    await assert.rejects(
      () =>
        guardReleaseMirrorUpload(
          'nyanpasu',
          'v1.2.3',
          { target: 'windows-x86_64', artifacts },
          async () => priorReleaseMirror(),
          async () => {
            uploads++
          },
          async () => {},
        ),
      { message: /immutable/ },
    )
    assert.deepStrictEqual(uploads, 0)
  }
})

test('release retry can repair a partial prior upload without replacing omitted bytes', async () => {
  let uploads = 0
  await guardReleaseMirrorUpload(
    'nyanpasu',
    'v1.2.3',
    {
      target: 'windows-x86_64',
      artifacts: releaseArtifacts.slice(0, -1),
    },
    async () => priorReleaseMirror(),
    async () => {
      uploads++
    },
    async () => {},
  )
  assert.deepStrictEqual(uploads, 1)
})

test('release upload proceeds without a sidecar but fails closed on lookup errors', async () => {
  let uploads = 0
  const candidate = { target: 'windows-x86_64', artifacts: releaseArtifacts }
  await guardReleaseMirrorUpload(
    'nyanpasu',
    'v1.2.3',
    candidate,
    async () => null,
    async () => {
      uploads++
    },
    async () => {},
  )
  assert.deepStrictEqual(uploads, 1)

  await assert.rejects(
    () =>
      guardReleaseMirrorUpload(
        'nyanpasu',
        'v1.2.3',
        candidate,
        async () => {
          throw new Error('GitHub API unavailable')
        },
        async () => {
          uploads++
        },
        async () => {},
      ),
    { message: /GitHub API unavailable/ },
  )
  assert.deepStrictEqual(uploads, 1)
})

test('release backfill retries accept the exact complete cross-platform inventory', async () => {
  let uploads = 0
  const prior = priorReleaseMirror(fullReleaseArtifacts)
  await guardReleaseMirrorUpload(
    'nyanpasu',
    'v1.2.3',
    { target: 'release-backfill', artifacts: fullReleaseArtifacts },
    async () => prior,
    async () => {
      uploads++
    },
    async () => {},
  )
  assert.deepStrictEqual(uploads, 1)

  await assert.rejects(
    () =>
      guardReleaseMirrorUpload(
        'nyanpasu',
        'v1.2.3',
        {
          target: 'release-backfill',
          artifacts: fullReleaseArtifacts.slice(0, -1),
        },
        async () => prior,
        async () => {
          uploads++
        },
        async () => {},
      ),
    { message: /immutable/ },
  )
  assert.deepStrictEqual(uploads, 1)
})

test('release reservations protect partial uploads before a GitHub sidecar exists', async () => {
  let inventory: unknown
  let uploads = 0
  const publish = (
    artifact: (typeof releaseArtifacts)[number],
    failUpload = false,
  ) =>
    guardReleaseMirrorUpload(
      'nyanpasu',
      'v1.2.3',
      { target: 'windows-x86_64', artifacts: [artifact] },
      async () => null,
      async () => {
        uploads++
        if (failUpload) {
          throw new Error('SFTP interrupted after partial upload')
        }
      },
      () =>
        reserveSourceforgeArtifact(
          artifact,
          async () => {
            if (inventory !== undefined) {
              throw new Error('Claim already exists')
            }
            inventory = { ...artifact }
          },
          async () => inventory,
        ),
    )
  await assert.rejects(() => publish(releaseArtifacts[0], true), {
    message: /interrupted/,
  })
  await assert.rejects(
    () => publish({ ...releaseArtifacts[0], sha256: 'f'.repeat(64) }),
    { message: /immutable/ },
  )
  assert.deepStrictEqual(uploads, 1)
  await publish(releaseArtifacts[0])
  assert.deepStrictEqual(uploads, 2)
})

test('incomplete or unreadable release reservations fail before uploading', async () => {
  for (const inventory of [
    null,
    {},
    { ...releaseArtifacts[0], fileSize: 99 },
  ]) {
    await assert.rejects(() =>
      reserveSourceforgeArtifact(
        releaseArtifacts[0],
        async () => {
          throw new Error('Claim already exists')
        },
        async () => inventory,
      ),
    )
  }
  await assert.rejects(
    () =>
      reserveSourceforgeArtifact(
        releaseArtifacts[0],
        async () => {
          throw new Error('Connection lost during claim creation')
        },
        async () => {
          throw new Error('Inventory unavailable')
        },
      ),
    { message: /Inventory unavailable/ },
  )
})

test('release reservations use SF-compatible directories and exclusive artifact claims', () => {
  const { batch, inventoryPath } = sourceforgeReservationBatch(
    'nyanpasu',
    'releases/v1.2.3',
    'Clash Nyanpasu.zip',
    '/tmp/inventory.json',
  )
  const mkdirs = batch
    .trim()
    .split('\n')
    .filter((line) => /^-?mkdir /.test(line))
  for (const line of mkdirs) {
    const directory = JSON.parse(line.slice(line.indexOf(' ') + 1)) as string
    for (const segment of directory.split('/').filter(Boolean)) {
      validateSourceforgeFilename(segment)
    }
  }
  assert.deepStrictEqual(
    inventoryPath,
    '/home/frs/project/nyanpasu/releases/v1.2.3/upload-inventory/Clash Nyanpasu.zip/inventory.json',
  )
  assert.deepStrictEqual(mkdirs.length, 4)
  assert.deepStrictEqual(
    mkdirs[2],
    '-mkdir "/home/frs/project/nyanpasu/releases/v1.2.3/upload-inventory"',
  )
  assert.deepStrictEqual(
    mkdirs[3],
    'mkdir "/home/frs/project/nyanpasu/releases/v1.2.3/upload-inventory/Clash Nyanpasu.zip"',
  )
  assert.deepStrictEqual(
    batch.includes('/upload-inventory/Clash Nyanpasu.zip/inventory.json"'),
    true,
  )
})

test('unreadable SF reservations retain creation and read failures', async () => {
  const creationError = new Error('SF rejected dot-prefixed directory')
  const readError = new Error('Inventory unavailable')
  await assert.rejects(
    () =>
      reserveSourceforgeArtifact(
        releaseArtifacts[0],
        async () => {
          throw creationError
        },
        async () => {
          throw readError
        },
      ),
    (error: Error) => {
      assert.match(error.message, /SF rejected dot-prefixed directory/)
      assert.ok(error.message.includes('Inventory unavailable'))
      assert.deepStrictEqual((error.cause as AggregateError).errors, [
        creationError,
        readError,
      ])
      return true
    },
  )
})
