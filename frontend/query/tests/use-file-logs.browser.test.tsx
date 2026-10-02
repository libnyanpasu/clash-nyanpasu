import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { Filter, LogPage, LogRow } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { useFileLogs } from '../src/ipc/use-file-logs'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

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

async function mountLogsHook(
  handlers: Record<string, (params?: Record<string, unknown>) => unknown>,
  onTestFinished: TestContext['onTestFinished'],
  useHook = () => useFileLogs('app', null, filter),
) {
  const testRpc = createTestRpc(handlers)
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  })
  const hook = await renderHook(useHook, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    testRpc.rpc.dispose()
    client.clear()
  })
  return { hook, testRpc }
}

test('a tail poll that finds no new rows does not re-render the viewer', async ({
  onTestFinished,
}) => {
  let renders = 0
  const queryLogs = vi.fn(async (params?: Record<string, unknown>) => {
    const { request } = params as { request: { direction: string } }
    return request.direction === 'latest'
      ? page([row(1), row(2)], 2)
      : // Every tail poll returns a fresh, empty page object.
        page([], 2)
  })
  const { hook } = await mountLogsHook(
    {
      list_log_files: async () => [
        { id: 'app.log', name: 'app.log', bytes: '100' },
      ],
      open_log_session: async () => ({ id: 'session', lease_ms: 60_000 }),
      close_log_session: async () => null,
      query_logs: queryLogs,
    },
    onTestFinished,
    () => {
      renders += 1
      return useFileLogs('app', null, filter)
    },
  )

  await expect.poll(() => hook.result.current.rows.length).toBe(2)
  const rows = hook.result.current.rows
  const polled = (count: number) =>
    expect
      .poll(() => queryLogs.mock.calls.length, { timeout: 5_000 })
      .toBeGreaterThanOrEqual(count)
  // React may run the component once more for the first identical state
  // before its eager bail-out applies; count from the first idle poll. A poll
  // is only scheduled once the previous one is applied, so the start of the
  // next call is what marks a poll as applied.
  const latest = queryLogs.mock.calls.length
  await polled(latest + 2)
  const settled = renders
  await polled(latest + 3)

  expect(hook.result.current.rows).toBe(rows)
  expect(renders).toBe(settled)
})

test('a tail poll appends new rows after the cached ones', async ({
  onTestFinished,
}) => {
  let tailPolls = 0
  const queryLogs = vi.fn(async (params?: Record<string, unknown>) => {
    const { request } = params as { request: { direction: string } }
    if (request.direction === 'latest') return page([row(1), row(2)], 2)
    tailPolls += 1
    return tailPolls === 1 ? page([row(3)], 3) : page([], 3)
  })
  const { hook } = await mountLogsHook(
    {
      list_log_files: async () => [
        { id: 'app.log', name: 'app.log', bytes: '100' },
      ],
      open_log_session: async () => ({ id: 'session', lease_ms: 60_000 }),
      close_log_session: async () => null,
      query_logs: queryLogs,
    },
    onTestFinished,
  )

  await expect
    .poll(() => hook.result.current.rows.map((r) => r.id), { timeout: 5_000 })
    .toEqual(['generation:1', 'generation:2', 'generation:3'])
})

test('a late rotation catalog cannot restore rows after follow-latest resets the view', async ({
  onTestFinished,
}) => {
  let finishCatalog!: (
    files: { id: string; name: string; bytes: string }[],
  ) => void
  let finishLatest!: (result: LogPage) => void
  let catalogCalls = 0
  let queries = 0
  const files = [{ id: 'rotated.log', name: 'rotated.log', bytes: '100' }]
  const { hook } = await mountLogsHook(
    {
      list_log_files: () => {
        catalogCalls += 1
        return catalogCalls === 2
          ? new Promise((resolve) => {
              finishCatalog = resolve
            })
          : files
      },
      open_log_session: () => ({ id: 'session', lease_ms: 60_000 }),
      close_log_session: () => null,
      query_logs: () => {
        queries += 1
        if (queries === 1) return page([row(1)], 1)
        if (queries === 2) return { ...page([row(2)], 2), file: 'rotated.log' }
        return new Promise((resolve) => {
          finishLatest = resolve
        })
      },
    },
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.rows.map((record) => record.id))
    .toEqual(['generation:1'])
  window.dispatchEvent(new Event('focus'))
  await expect.poll(() => catalogCalls).toBe(2)
  hook.result.current.latest()
  await expect.poll(() => hook.result.current.rows.length).toBe(0)
  finishCatalog(files)
  await expect.poll(() => queries, { timeout: 5_000 }).toBe(3)
  expect(hook.result.current.rows).toEqual([])
  finishLatest({ ...page([row(3)], 3), file: 'rotated.log' })
  await expect
    .poll(() => hook.result.current.rows.map((record) => record.id))
    .toEqual(['generation:3'])
})
