import { expect, test } from 'vitest'
import {
  estimateCoreLogRowBytes,
  MAX_CORE_LOG_BYTES,
  MAX_CORE_LOG_ROWS,
  mergeCoreLogRows,
} from '../src/ipc/core-log-viewer-state'
import type {
  CoreLogCursor,
  CoreLogRow,
  CoreLogStatus,
} from '../src/ipc/rpc-bindings'

const cursor = (sequence: number, generation = 'test'): CoreLogCursor => ({
  generation,
  segment: 1,
  sequence,
})
const row = (sequence: number, bytes = 32): CoreLogRow => ({
  id: cursor(sequence),
  record: {
    source: {
      capture: 'capture',
      instance_id: 'instance',
      core_kind: 'Mihomo',
    },
    type: 'debug',
    time: null,
    received_at: sequence,
    payload: 'x'.repeat(bytes),
  },
  truncated: false,
})
const status = (
  first = 1,
  head = 10000,
  generation = 'test',
): CoreLogStatus => ({
  generation,
  version: 1,
  first: cursor(first, generation),
  head: cursor(head, generation),
  bytes: 1024,
  budget: 64 * 1024 * 1024,
  error: null,
  discarded: 0,
})

test('row count and byte budget both limit the page cache', () => {
  const all = Array.from({ length: 10000 }, (_, index) => row(index + 1))
  const small = mergeCoreLogRows([], all, status())
  expect(small).toHaveLength(MAX_CORE_LOG_ROWS)
  expect(small[0].id.sequence).toBe(9501)
  const large = mergeCoreLogRows(
    [],
    all.slice(0, 1000).map((r) => row(r.id.sequence, 4096)),
    status(),
  )
  expect(large.length).toBeLessThan(MAX_CORE_LOG_ROWS)
  expect(
    large.reduce((bytes, r) => bytes + estimateCoreLogRowBytes(r), 0),
  ).toBeLessThanOrEqual(MAX_CORE_LOG_BYTES)
})

test('rolling and clear invalidate retained rows even without an incoming page', () => {
  const rows = [row(1), row(2), row(3)]
  expect(mergeCoreLogRows(rows, [], status(2))).toEqual(rows.slice(1))
  expect(mergeCoreLogRows(rows, [], status(1, 10000, 'cleared'))).toEqual([])
  expect(
    mergeCoreLogRows(rows, [], { ...status(), first: null, head: null }),
  ).toEqual([])
})

test('overlapping pages deduplicate and older navigation retains the earlier range', () => {
  const current = Array.from({ length: 500 }, (_, index) => row(index + 201))
  const incoming = Array.from({ length: 300 }, (_, index) => row(index + 1))
  const merged = mergeCoreLogRows(current, incoming, status(), true)
  expect(merged).toHaveLength(500)
  expect(merged[0].id.sequence).toBe(1)
  expect(merged.at(-1)?.id.sequence).toBe(500)
  expect(mergeCoreLogRows(merged, [], status())).toBe(merged)
})
