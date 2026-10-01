import { expect, test } from 'vitest'
import {
  advanceCursor,
  LOG_CACHE_ROWS,
  mergeLogRows,
} from '../src/ipc/log-viewer-state.ts'
import type { LogRow } from '../src/ipc/rpc-bindings'

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
  const rows = mergeLogRows(
    [row(9), row(12)],
    [row(100), row(12), row(3)],
    null,
  )
  expect(rows.map((r) => r.id)).toEqual([
    'generation:3',
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
test('a later page replaces a retried row and floors only its generation', () => {
  const floor = { generation: 'generation', offset: '20' }
  const rotated = { ...row(5), id: 'rotated:5' }
  const rows = mergeLogRows(
    [row(20, 'old'), row(30)],
    [row(20, 'new'), rotated, row(10), row(40)],
    floor,
  )
  expect(rows.map((r) => r.id)).toEqual([
    'rotated:5',
    'generation:20',
    'generation:30',
    'generation:40',
  ])
  expect(rows[1].raw).toBe('new')
})
test('an out-of-order page is merged into position order', () => {
  const rows = mergeLogRows(
    [row(10), row(30)],
    [row(40), row(20), row(30), row(5)],
    null,
  )
  expect(rows.map((r) => r.id)).toEqual([
    'generation:5',
    'generation:10',
    'generation:20',
    'generation:30',
    'generation:40',
  ])
})
test('appending past the cache budget drops the oldest rows', () => {
  const rows = mergeLogRows(
    Array.from({ length: LOG_CACHE_ROWS }, (_, n) => row(n)),
    [row(LOG_CACHE_ROWS), row(LOG_CACHE_ROWS + 1)],
    null,
  )
  expect(rows.length).toBe(LOG_CACHE_ROWS)
  expect(rows[0].id).toBe('generation:2')
  expect(rows.at(-1)?.id).toBe(`generation:${LOG_CACHE_ROWS + 1}`)
})
test('the byte budget keeps the newest rows of a tail page', () => {
  const rows = mergeLogRows(
    [],
    Array.from({ length: 100 }, (_, n) => row(n, 'x'.repeat(100_000))),
    null,
  )
  expect(rows.at(-1)?.id).toBe('generation:99')
  expect(rows[0].id).toBe(`generation:${100 - rows.length}`)
})
test('an empty page keeps the rows array', () => {
  const previous = [row(1), row(2)]
  expect(mergeLogRows(previous, [], null)).toBe(previous)
  expect(mergeLogRows(previous, [], null, true)).toBe(previous)
})
