import { beforeEach, expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type {
  CoreLogCursor,
  CoreLogQuery,
  CoreLogRow,
  CoreLogStatus,
} from '@nyanpasu/rpc/types'
import { useClashLogs } from '../src/ipc/use-clash-logs'

const mocked = vi.hoisted(() => ({
  queryCoreLogs: vi.fn(),
  getCoreLogStatus: vi.fn(),
  getCoreLog: vi.fn(),
  clearCoreLogs: vi.fn(),
  listeners: new Set<(event: { payload: { status: CoreLogStatus } }) => void>(),
  resync: new Set<() => void>(),
}))
vi.mock('../src/provider/rpc-provider', () => {
  const rpc = {
    ...mocked,
    events: {
      coreLogsChanged: {
        listen: async (
          listener: (event: { payload: { status: CoreLogStatus } }) => void,
        ) => {
          mocked.listeners.add(listener)
          return () => mocked.listeners.delete(listener)
        },
      },
    },
    listenResync: (listener: () => void) => {
      mocked.resync.add(listener)
      return () => mocked.resync.delete(listener)
    },
  }
  return { useRpc: () => rpc }
})

let generation: string
let head: number
let version: number
const ok = <T,>(data: T) => ({ status: 'ok' as const, data })
const cursor = (sequence: number): CoreLogCursor => ({
  generation,
  sequence,
})
const status = (): CoreLogStatus => ({
  generation,
  version,
  first: head ? cursor(1) : null,
  head: head ? cursor(head) : null,
  bytes: head * 100,
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
  version++
  for (const listener of mocked.listeners)
    listener({ payload: { status: status() } })
}
beforeEach(() => {
  vi.clearAllMocks()
  generation = 'first'
  version = 1
  head = 3
  mocked.getCoreLogStatus.mockImplementation(async () => ok(status()))
  mocked.queryCoreLogs.mockImplementation(async (query: CoreLogQuery) => {
    expect(mocked.listeners.size).toBeGreaterThan(0)
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

test('subscribes before its first query and stays idle without polling or changing references', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() => useClashLogs())
  onTestFinished(() => hook.unmount())
  await expect.poll(() => hook.result.current.data.length).toBe(1)
  expect(mocked.listeners.size).toBe(1)
  expect(mocked.getCoreLogStatus).not.toHaveBeenCalled()
  expect(mocked.queryCoreLogs).toHaveBeenCalledOnce()
  const view = hook.result.current
  for (const listener of mocked.listeners)
    listener({ payload: { status: status() } })
  vi.useFakeTimers()
  try {
    await vi.advanceTimersByTimeAsync(10000)
  } finally {
    vi.useRealTimers()
  }
  expect(mocked.queryCoreLogs).toHaveBeenCalledOnce()
  expect(mocked.getCoreLogStatus).not.toHaveBeenCalled()
  expect(hook.result.current).toBe(view)
})

test('coalesces a burst into one catch-up query without extra status RPCs', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() => useClashLogs())
  onTestFinished(() => hook.unmount())
  await expect.poll(() => hook.result.current.data.length).toBe(1)
  let finish!: () => void
  mocked.queryCoreLogs.mockImplementationOnce(() => {
    const page = {
      rows: [row(head)],
      cursor: cursor(head),
      more: false,
      status: status(),
    }
    return new Promise((resolve) => {
      finish = () => resolve(ok(page))
    })
  })
  head = 4
  changed()
  for (head = 5; head <= 103; head++) changed()
  head = 103
  expect(mocked.queryCoreLogs).toHaveBeenCalledTimes(2)
  finish()
  await expect
    .poll(() => hook.result.current.data.at(-1)?.id.sequence)
    .toBe(103)
  expect(mocked.queryCoreLogs).toHaveBeenCalledTimes(3)
  expect(mocked.getCoreLogStatus).not.toHaveBeenCalled()
})

test('hidden pages trim evicted previews without fetching bodies and resync when visible', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() => useClashLogs())
  const hidden = vi.spyOn(document, 'hidden', 'get')
  onTestFinished(async () => {
    hidden.mockRestore()
    await hook.unmount()
  })
  await expect.poll(() => hook.result.current.data.length).toBe(1)
  hidden.mockReturnValue(true)
  head = 10
  version++
  for (const listener of mocked.listeners)
    listener({ payload: { status: { ...status(), first: cursor(8) } } })
  await expect.poll(() => hook.result.current.data.length).toBe(0)
  expect(mocked.queryCoreLogs).toHaveBeenCalledOnce()
  generation = 'cleared-while-hidden'
  head = 0
  changed()
  await expect.poll(() => hook.result.current.isLoading).toBe(false)
  expect(mocked.queryCoreLogs).toHaveBeenCalledOnce()
  head = 10
  changed()
  hidden.mockReturnValue(false)
  document.dispatchEvent(new Event('visibilitychange'))
  await expect.poll(() => hook.result.current.data.at(-1)?.id.sequence).toBe(10)
  expect(mocked.getCoreLogStatus).toHaveBeenCalledOnce()
})

test('an old in-flight page cannot restore records after a clear event', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() => useClashLogs())
  onTestFinished(() => hook.unmount())
  await expect.poll(() => hook.result.current.data.length).toBe(1)
  let finish!: () => void
  mocked.queryCoreLogs.mockImplementationOnce(() => {
    const page = {
      rows: [row(head)],
      cursor: cursor(head),
      more: false,
      status: status(),
    }
    return new Promise((resolve) => {
      finish = () => resolve(ok(page))
    })
  })
  head = 4
  changed()
  generation = 'replacement'
  head = 1
  changed()
  finish()
  await expect
    .poll(() => hook.result.current.data.map((row) => row.id))
    .toEqual([cursor(1)])
  expect(hook.result.current.status?.generation).toBe('replacement')
})

test('transport resync and an expired cursor recover through a fresh page', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() => useClashLogs())
  onTestFinished(async () => {
    await hook.unmount()
    expect(mocked.resync.size).toBe(0)
  })
  await expect.poll(() => hook.result.current.data.length).toBe(1)
  mocked.queryCoreLogs.mockResolvedValueOnce({
    status: 'error',
    error: { kind: 'cursor_expired' },
  })
  head = 20
  version++
  for (const listener of mocked.resync) listener()
  await expect.poll(() => hook.result.current.data.at(-1)?.id.sequence).toBe(20)
  expect(mocked.queryCoreLogs.mock.lastCall?.[0].direction).toBe('latest')
  expect(mocked.getCoreLogStatus).toHaveBeenCalledOnce()
})
