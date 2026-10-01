import { expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { Filter, LogPage, LogRow } from '../src/ipc/rpc-bindings'
import { useFileLogs } from '../src/ipc/use-file-logs'

const rpc = vi.hoisted(() => ({
  listLogFiles: vi.fn(),
  openLogSession: vi.fn(),
  queryLogs: vi.fn(),
  closeLogSession: vi.fn(),
}))
vi.mock('../src/ipc/rpc', () => ({ rpc }))

const ok = <T,>(data: T) => ({ status: 'ok' as const, data })
const row = (offset: number): LogRow => ({
  id: `generation:${offset}`,
  timestamp: null,
  level: 'info',
  target: '',
  message: `message ${offset}`,
  raw: `message ${offset}`,
  unparsed: false,
  truncated: false,
})
const cursor = (offset: number) => ({
  generation: 'generation',
  offset: String(offset),
})
const page = (rows: LogRow[], offset: number): LogPage => ({
  rows,
  cursor: cursor(offset),
  head: cursor(offset),
  start: '0',
  file: 'app.log',
  building: false,
  more: false,
  partial: false,
  malformed: '0',
  truncated: '0',
  indexed_bytes: '100',
  file_bytes: '100',
})
const filter: Filter = {
  levels: [],
  target: null,
  text: null,
  from_ms: null,
  to_ms: null,
}

test('a tail poll that finds no new rows does not re-render the viewer', async ({
  onTestFinished,
}) => {
  rpc.listLogFiles.mockResolvedValue(
    ok([{ id: 'app.log', name: 'app.log', bytes: '100' }]),
  )
  rpc.openLogSession.mockResolvedValue(ok({ id: 'session', lease_ms: 60_000 }))
  rpc.closeLogSession.mockResolvedValue(ok(null))
  rpc.queryLogs.mockImplementation(async (_source, request) =>
    ok(
      request.direction === 'latest'
        ? page([row(1), row(2)], 2)
        : // Every tail poll returns a fresh, empty page object.
          page([], 2),
    ),
  )
  let renders = 0
  const hook = await renderHook(() => {
    renders += 1
    return useFileLogs('app', null, filter)
  })
  onTestFinished(() => hook.unmount())

  await expect.poll(() => hook.result.current.rows.length).toBe(2)
  const rows = hook.result.current.rows
  const polled = (count: number) =>
    expect
      .poll(() => rpc.queryLogs.mock.calls.length, { timeout: 5_000 })
      .toBeGreaterThanOrEqual(count)
  // React may run the component once more for the first identical state
  // before its eager bail-out applies; count from the first idle poll.
  await polled(rpc.queryLogs.mock.calls.length + 1)
  const settled = renders
  await polled(rpc.queryLogs.mock.calls.length + 2)

  expect(hook.result.current.rows).toBe(rows)
  expect(renders).toBe(settled)
})

test('a tail poll appends new rows after the cached ones', async ({
  onTestFinished,
}) => {
  rpc.listLogFiles.mockResolvedValue(
    ok([{ id: 'app.log', name: 'app.log', bytes: '100' }]),
  )
  rpc.openLogSession.mockResolvedValue(ok({ id: 'session', lease_ms: 60_000 }))
  rpc.closeLogSession.mockResolvedValue(ok(null))
  let tailPolls = 0
  rpc.queryLogs.mockImplementation(async (_source, request) => {
    if (request.direction === 'latest') return ok(page([row(1), row(2)], 2))
    tailPolls += 1
    return ok(tailPolls === 1 ? page([row(3)], 3) : page([], 3))
  })
  const hook = await renderHook(() => useFileLogs('app', null, filter))
  onTestFinished(() => hook.unmount())

  await expect
    .poll(() => hook.result.current.rows.map((r) => r.id), { timeout: 5_000 })
    .toEqual(['generation:1', 'generation:2', 'generation:3'])
})
