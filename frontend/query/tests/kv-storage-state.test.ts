import { expect, test } from 'vitest'
import {
  EMPTY_KV_STORAGE_ENTRY,
  kvStorageEventSnapshot,
  mergeKvStorageSnapshot,
  type KvStorageEntry,
} from '../src/provider/kv-storage-state'

const entry = (patch: Partial<KvStorageEntry>): KvStorageEntry => ({
  ...EMPTY_KV_STORAGE_ENTRY,
  ...patch,
})

test('only a newer revision replaces the committed value', () => {
  const current = entry({ value: '"b"', revision: 5 })

  expect(mergeKvStorageSnapshot(current, { value: '"a"', revision: 4 })).toBe(
    current,
  )
  expect(mergeKvStorageSnapshot(current, { value: '"a"', revision: 5 })).toBe(
    current,
  )
  expect(
    mergeKvStorageSnapshot(current, { value: null, revision: 6 }),
  ).toMatchObject({ value: null, revision: 6 })
})

test('the first snapshot is accepted at revision zero', () => {
  expect(
    mergeKvStorageSnapshot(EMPTY_KV_STORAGE_ENTRY, {
      value: '1',
      revision: 0,
    }),
  ).toMatchObject({ value: '1', revision: 0 })
})

test('a newer commit replaces a failed draft but not a pending one', () => {
  const snapshot = { value: '"remote"', revision: 2 }
  const pending = entry({
    revision: 1,
    draft: { id: 1, value: '"local"', failed: false },
  })
  const failed = entry({
    revision: 1,
    draft: { id: 1, value: '"local"', failed: true },
  })

  expect(mergeKvStorageSnapshot(pending, snapshot).draft).toBe(pending.draft)
  expect(mergeKvStorageSnapshot(failed, snapshot).draft).toBeNull()
  // An older or equal snapshot leaves even a failed draft shown.
  expect(
    mergeKvStorageSnapshot(failed, { value: '"old"', revision: 1 }).draft,
  ).toBe(failed.draft)
})

test('events of frontend keys decode the stored JSON text once', () => {
  expect(
    kvStorageEventSnapshot({
      key: 'web:layout',
      value: JSON.stringify(JSON.stringify({ a: 1 })),
      revision: 3,
    }),
  ).toEqual({
    key: 'layout',
    snapshot: { value: '{"a":1}', revision: 3 },
  })
  // A stored JSON null is a value, not a removal.
  expect(
    kvStorageEventSnapshot({ key: 'web:css', value: '"null"', revision: 4 }),
  ).toEqual({ key: 'css', snapshot: { value: 'null', revision: 4 } })
  expect(
    kvStorageEventSnapshot({ key: 'web:css', value: null, revision: 5 }),
  ).toEqual({ key: 'css', snapshot: { value: null, revision: 5 } })
})

test('internal keys and malformed values are ignored', () => {
  expect(
    kvStorageEventSnapshot({ key: 'tasks', value: '"x"', revision: 1 }),
  ).toBeNull()
  expect(
    kvStorageEventSnapshot({ key: 'web:a', value: '{', revision: 1 }),
  ).toBeNull()
  expect(
    kvStorageEventSnapshot({ key: 'web:a', value: '1', revision: 1 }),
  ).toBeNull()
  // The stored text itself is not JSON.
  expect(
    kvStorageEventSnapshot({
      key: 'web:a',
      value: JSON.stringify('{'),
      revision: 1,
    }),
  ).toBeNull()
})
