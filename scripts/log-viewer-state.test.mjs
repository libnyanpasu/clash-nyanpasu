import assert from 'node:assert/strict'
import test from 'node:test'
import { advanceCursor, mergeLogRows, LOG_CACHE_ROWS } from '../frontend/interface/src/ipc/log-viewer-state.ts'

const row = (offset, raw = '') => ({ id: `generation:${offset}`, timestamp: null, level: 'info', target: '', message: raw, raw, unparsed: false, truncated: false })
test('physical ordering and retry deduplication do not depend on timestamps', () => {
  const rows = mergeLogRows([row(12), row(9)], [row(12), row(100)], null)
  assert.deepEqual(rows.map((r) => r.id), ['generation:9', 'generation:12', 'generation:100'])
})
test('clear floor excludes delayed history and advances a stale live cursor', () => {
  const floor = { generation: 'generation', offset: '20' }
  assert.deepEqual(mergeLogRows([], [row(10), row(20), row(30)], floor).map((r) => r.id), ['generation:20', 'generation:30'])
  assert.deepEqual(advanceCursor({ generation: 'generation', offset: '10' }, floor), floor)
})
test('cache bounds both row count and long messages', () => {
  assert.equal(mergeLogRows([], Array.from({ length: LOG_CACHE_ROWS + 100 }, (_, n) => row(n)), null).length, LOG_CACHE_ROWS)
  assert.ok(mergeLogRows([], Array.from({ length: 100 }, (_, n) => row(n, 'x'.repeat(100_000))), null).length < 20)
})
test('prepending retains the requested older region within the cache budget', () => {
  const rows = mergeLogRows(Array.from({ length: LOG_CACHE_ROWS }, (_, n) => row(n + 100)), [row(1)], null, true)
  assert.equal(rows[0].id, 'generation:1')
  assert.equal(rows.length, LOG_CACHE_ROWS)
})
