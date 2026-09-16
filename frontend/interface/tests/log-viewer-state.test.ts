import { expect, test } from 'vitest'
import type { LogRow } from '../src/ipc/bindings'
import {
  advanceCursor,
  LOG_CACHE_ROWS,
  mergeLogRows,
} from '../src/ipc/log-viewer-state.ts'

const row = (offset: number, raw = ''): LogRow => ({
  id: `generation:${offset}`,
  timestamp: null,
  level: 'info',
  target: '',
  message: raw,
  raw,
  unparsed: false,
  truncated: false,
})
test('physical ordering and retry deduplication do not depend on timestamps', () => {
  const rows = mergeLogRows([row(12), row(9)], [row(12), row(100)], null)
  expect(rows.map((r) => r.id)).toEqual([
    'generation:9',
    'generation:12',
    'generation:100',
  ])
})
test('clear floor excludes delayed history and advances a stale live cursor', () => {
  const floor = { generation: 'generation', offset: '20' }
  expect(
    mergeLogRows([], [row(10), row(20), row(30)], floor).map((r) => r.id),
  ).toEqual(['generation:20', 'generation:30'])
  expect(
    advanceCursor({ generation: 'generation', offset: '10' }, floor),
  ).toEqual(floor)
})
test('cache bounds both row count and long messages', () => {
  expect(
    mergeLogRows(
      [],
      Array.from({ length: LOG_CACHE_ROWS + 100 }, (_, n) => row(n)),
      null,
    ).length,
  ).toBe(LOG_CACHE_ROWS)
  expect(
    mergeLogRows(
      [],
      Array.from({ length: 100 }, (_, n) => row(n, 'x'.repeat(100_000))),
      null,
    ).length < 20,
  ).toBeTruthy()
})
test('prepending retains the requested older region within the cache budget', () => {
  const rows = mergeLogRows(
    Array.from({ length: LOG_CACHE_ROWS }, (_, n) => row(n + 100)),
    [row(1)],
    null,
    true,
  )
  expect(rows[0].id).toBe('generation:1')
  expect(rows.length).toBe(LOG_CACHE_ROWS)
})
