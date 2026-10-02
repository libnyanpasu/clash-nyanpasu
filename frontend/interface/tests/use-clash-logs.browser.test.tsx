import { beforeEach, expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type {
  CoreLogCursor,
  CoreLogQuery,
  CoreLogRow,
  CoreLogStatus,
} from '../src/ipc/rpc-bindings'
import { useClashLogs } from '../src/ipc/use-clash-logs'

const mocked = vi.hoisted(() => ({
  queryCoreLogs: vi.fn(),
  getCoreLogStatus: vi.fn(),
  getCoreLog: vi.fn(),
  clearCoreLogs: vi.fn(),
  listeners: new Set<() => void>(),
}))
vi.mock('../src/ipc/rpc', () => ({
  rpc: {
    ...mocked,
    events: {
      coreLogsChanged: {
        listen: async (listener: () => void) => {
          mocked.listeners.add(listener)
          return () => mocked.listeners.delete(listener)
        },
      },
    },
    listenResync: () => () => {},
  },
}))

let generation: string
let head: number
const ok = <T,>(data: T) => ({ status: 'ok' as const, data })
const cursor = (sequence: number): CoreLogCursor => ({
  generation,
  segment: 1,
  sequence,
})
const status = (): CoreLogStatus => ({
  generation,
  version: head,
  first: head ? cursor(1) : null,
  head: head ? cursor(head) : null,
  bytes: head * 100,
  budget: 64 * 1024 * 1024,
  error: null,
  discarded: 0,
})
const row = (sequence: number): CoreLogRow => ({
  id: cursor(sequence),
  truncated: true,
  record: {
    source: {
      capture: 'capture',
      instance_id: 'instance',
      core_kind: 'Mihomo',
    },
    received_at: sequence,
    time: null,
    type: 'debug',
    payload: `preview ${sequence}`,
  },
})
const changed = () => {
  for (const listener of mocked.listeners) listener()
}
beforeEach(() => {
  vi.clearAllMocks()
  generation = 'first'
  head = 3
  mocked.getCoreLogStatus.mockImplementation(async () => ok(status()))
  mocked.queryCoreLogs.mockImplementation(async (query: CoreLogQuery) => {
    const rows =
      query.direction === 'latest'
        ? [row(head)]
        : query.direction === 'before'
          ? [row(1), row(2)]
          : head > (query.cursor?.sequence ?? 0)
            ? [row(head)]
            : []
    return ok({
      rows,
      cursor: query.direction === 'before' ? cursor(1) : cursor(head),
      more: query.direction === 'latest',
      status: status(),
    })
  })
  mocked.clearCoreLogs.mockImplementation(async () => {
    generation = 'cleared'
    head = 0
    changed()
    return ok(null)
  })
  mocked.getCoreLog.mockImplementation(async (id: CoreLogCursor) =>
    ok({ ...row(id.sequence).record, payload: 'complete record' }),
  )
})

test('pause leaves capture status live, older rows load on demand, and details bypass previews', async ({
  onTestFinished,
}) => {
  const hook = await renderHook<
    { following: boolean },
    ReturnType<typeof useClashLogs>
  >(
    ({ following } = { following: true }) =>
      useClashLogs('debug', 'preview', following),
    { initialProps: { following: true } },
  )
  onTestFinished(async () => {
    await hook.unmount()
    expect(mocked.listeners.size).toBe(0)
  })
  await expect
    .poll(() => hook.result.current.data.map((r) => r.id.sequence))
    .toEqual([3])
  expect(mocked.queryCoreLogs.mock.lastCall?.[0]).toMatchObject({
    level: 'debug',
    keyword: 'preview',
  })
  await hook.rerender({ following: false })
  head = 4
  changed()
  await expect.poll(() => hook.result.current.status?.head?.sequence).toBe(4)
  expect(hook.result.current.data.map((r) => r.id.sequence)).toEqual([3])
  hook.result.current.loadOlder()
  await expect
    .poll(() => hook.result.current.data.map((r) => r.id.sequence))
    .toEqual([1, 2, 3])
  expect((await hook.result.current.detail(cursor(3))).payload).toBe(
    'complete record',
  )
  await hook.rerender({ following: true })
  await expect
    .poll(() => hook.result.current.data.map((r) => r.id.sequence))
    .toEqual([4])
})

test('clearing in one mounted viewer invalidates the other viewer history', async ({
  onTestFinished,
}) => {
  const first = await renderHook(() => useClashLogs())
  const second = await renderHook(() => useClashLogs())
  onTestFinished(async () => {
    await first.unmount()
    await second.unmount()
    expect(mocked.listeners.size).toBe(0)
  })
  await expect.poll(() => first.result.current.data.length).toBe(1)
  await expect.poll(() => second.result.current.data.length).toBe(1)
  mocked.queryCoreLogs.mockImplementation(async () =>
    ok({ rows: [], cursor: null, more: false, status: status() }),
  )
  await first.result.current.clean.mutateAsync()
  await expect
    .poll(() => second.result.current.status?.generation)
    .toBe('cleared')
  expect(second.result.current.data).toEqual([])
  expect(mocked.clearCoreLogs).toHaveBeenCalledOnce()
})
