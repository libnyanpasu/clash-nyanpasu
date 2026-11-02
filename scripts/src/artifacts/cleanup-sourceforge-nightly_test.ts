import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  archiveReadyTargets,
  parseManagedNightlyDirectories,
  selectNightlyCleanupTargets,
} from './cleanup-sourceforge-nightly.ts'

const oldBuild = '120-1-' + 'a'.repeat(40)
const currentBuild = '121-2-' + 'b'.repeat(40)
const newerBuild = '122-1-' + 'c'.repeat(40)

test('SourceForge cleanup only recognizes immutable managed nightly IDs', () => {
  assert.deepStrictEqual(
    parseManagedNightlyDirectories(`${oldBuild}\n${currentBuild}\n`),
    [oldBuild, currentBuild],
  )
  assert.throws(() => parseManagedNightlyDirectories(`${oldBuild}\nlatest\n`), {
    message: /Unexpected entry/,
  })
})

test('nightly cleanup requires every archived target to be ready before deleting an old folder', async () => {
  const allReady = await archiveReadyTargets(
    currentBuild,
    'token',
    async (input) => {
      const buildId = String(input).split('/').at(-1)!
      return new Response(JSON.stringify({ buildId, status: 'ready' }))
    },
  )
  assert.deepStrictEqual(allReady.ready, true)
  assert.deepStrictEqual(allReady.missing, [])

  const pending = await archiveReadyTargets(
    currentBuild,
    'token',
    async (input) => {
      const buildId = String(input).split('/').at(-1)!
      return new Response(JSON.stringify({ buildId, status: 'pending' }))
    },
  )
  assert.deepStrictEqual(pending.ready, false)
  assert.deepStrictEqual(pending.missing.length, 6)

  const requests: string[] = []
  const legacy = await archiveReadyTargets(
    currentBuild,
    'token',
    async (input) => {
      const buildId = String(input).split('/').at(-1)!
      requests.push(buildId)
      return buildId.startsWith('tg-')
        ? new Response(null, { status: 404 })
        : Response.json({ buildId, status: 'ready' })
    },
  )
  assert.deepStrictEqual(legacy.ready, true)
  assert.deepStrictEqual(requests.length, 12)
  assert.deepStrictEqual(requests[0], `tg-${currentBuild}-windows-x86_64`)
})

test('nightly cleanup retains the promoted newest build and removes older managed builds', () => {
  assert.deepStrictEqual(
    selectNightlyCleanupTargets([oldBuild, currentBuild], currentBuild),
    [oldBuild],
  )
  assert.deepStrictEqual(
    selectNightlyCleanupTargets([currentBuild], currentBuild),
    [],
  )
  assert.deepStrictEqual(
    selectNightlyCleanupTargets(
      [oldBuild, currentBuild, newerBuild],
      currentBuild,
    ),
    null,
  )
  assert.throws(() => selectNightlyCleanupTargets([oldBuild], currentBuild), {
    message: /directory is missing/,
  })
})
