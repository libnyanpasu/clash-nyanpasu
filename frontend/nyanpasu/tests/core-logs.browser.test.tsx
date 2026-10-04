import type { ComponentType, PropsWithChildren } from 'react'
import { beforeEach, expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { createHttpEventTransport, createRpcClient } from '@nyanpasu/rpc'
import type {
  CoreStatusInfo,
  LogError,
  LogPage,
  LogRow,
  QueryLogs,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { Route } from '../src/pages/(main)/main/logs'
import { m } from '../src/paraglide/messages'
import { TestQueryProvider } from './query-provider'

const mocked = vi.hoisted(() => ({
  navigate: vi.fn(),
  source: 'core' as 'core' | 'app' | 'service',
}))

vi.mock('../src/pages/(main)/main/logs/route', () => ({
  Route: {
    useSearch: () => ({ source: mocked.source }),
    useNavigate: () => mocked.navigate,
  },
  LogLevelsLayout: ({ children }: PropsWithChildren) => children,
}))

const CoreLogsPage = Route.options.component as ComponentType
const cursor = (offset: number, generation = 'archive') => ({
  generation,
  offset: String(offset),
})
const rows = (start: number, count: number, generation = 'archive'): LogRow[] =>
  Array.from({ length: count }, (_, index) => ({
    id: `${generation}:${start + index}`,
    timestamp: String(1_700_000_000_000 + start + index),
    level: 'info',
    target: 'mihomo',
    message: `record ${start + index}`,
    raw: JSON.stringify({
      t: 'log',
      at: 1_700_000_000_000 + start + index,
      epoch: 9,
      kind: 'mihomo',
      stream: 'stderr',
      level: 'info',
      timestamp: { raw: '12:00:00', unix_ms: null, inferred: true },
      target: 'mihomo',
      message: `record ${start + index}`,
      fields: [],
      raw: `record ${start + index}`,
      truncated: false,
    }),
    unparsed: false,
    truncated: false,
  }))
const page = (
  records = rows(201, 20),
  offset = 201,
  more = true,
  generation = 'archive',
): LogPage => ({
  rows: records,
  cursor: cursor(offset, generation),
  head: cursor(221, generation),
  start: '0',
  file: 'core-000002.jsonl',
  building: false,
  more,
  partial: false,
  malformed: '0',
  truncated: '0',
  indexed_bytes: '22000',
  file_bytes: '22000',
})
const status = (host: 'local' | 'service' = 'local'): CoreStatusInfo => ({
  host,
  state: { Stopped: { reason: 'core exited' } },
  controller: null,
  connectivity: { kind: 'shut_down' },
  state_changed_at: 1,
  generation: 1,
  revision: null,
  healthy: null,
})

type Handler = (params?: Record<string, unknown>) => unknown | Promise<unknown>

function logFailure(error: LogError): Promise<never> {
  // The RPC transport rejects with the serialized unit-enum domain error.
  // oxlint-disable-next-line eslint(prefer-promise-reject-errors)
  return Promise.reject(error)
}
async function mountPage(
  onTestFinished: TestContext['onTestFinished'],
  overrides: Record<string, Handler> = {},
) {
  const handlers: Record<string, Handler> = {
    get_core_status: () => status(),
    list_log_files: () => [
      { id: 'core-000001.jsonl', name: 'core-000001.jsonl', bytes: '100' },
      { id: 'core-000002.jsonl', name: 'core-000002.jsonl', bytes: '22000' },
    ],
    open_log_session: () => ({ id: crypto.randomUUID(), lease_ms: 60_000 }),
    close_log_session: () => null,
    query_logs: (params) => {
      const { request } = params as { request: QueryLogs }
      return request.direction === 'latest' ? page() : page([], 221, false)
    },
    ...overrides,
  }
  const invoke = vi.fn(
    async (method: string, params?: Record<string, unknown>) => {
      if (!handlers[method])
        throw new Error(`Unexpected RPC command: ${method}`)
      return handlers[method](params)
    },
  )
  const rpc = createRpcClient({
    commands: {
      invoke: async <T,>(method: string, params?: Record<string, unknown>) =>
        (await invoke(method, params)) as T,
    },
    events: createHttpEventTransport(),
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  })
  const view = await render(
    <TestQueryProvider client={client} rpc={rpc}>
      <style>{`
        [data-slot="scroll-area"] { height: 330px; overflow: hidden; }
        [data-slot="scroll-area-viewport"] { height: 330px; }
        [data-slot="logs-virtual-list"] { position: relative; }
        [data-slot="logs-virtual-item"] { position: absolute; top: 0; width: 100%; height: 110px; }
      `}</style>
      <CoreLogsPage />
    </TestQueryProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    rpc.dispose()
    client.clear()
  })
  const calls = (method: string) =>
    invoke.mock.calls
      .filter(([name]) => name === method)
      .map(([, params]) => params!)
  const queryCalls = () =>
    calls('query_logs').map((params) => params.request as QueryLogs)
  return { view, client, calls, queryCalls }
}

beforeEach(() => {
  mocked.source = 'core'
  vi.clearAllMocks()
})

test.for(['core', 'app', 'service'] as const)(
  '%s footer selects current and archived files through the MDY select',
  async (source, { onTestFinished }) => {
    mocked.source = source
    const { view, calls } = await mountPage(onTestFinished)
    const rpcSource = source === 'core' ? 'core_local' : source
    await expect.poll(() => calls('open_log_session').length).toBe(1)
    const select = view.getByRole('combobox', { name: m.logs_file_label() })
    expect(select.element().textContent).toContain(m.logs_current_file())
    await select.click()
    await view.getByRole('option', { name: 'core-000001.jsonl' }).click()
    await expect
      .poll(() => calls('open_log_session').at(-1))
      .toMatchObject({
        source: rpcSource,
        request: { file: 'core-000001.jsonl' },
      })
    expect(select.element().textContent).toContain('core-000001.jsonl')
    await select.click()
    await view.getByRole('option', { name: m.logs_current_file() }).click()
    await expect
      .poll(() => calls('open_log_session').at(-1))
      .toMatchObject({
        source: rpcSource,
        request: { file: null },
      })
  },
)

test('stopped Core archives have no zero-diagnostic header or stray text', async ({
  onTestFinished,
}) => {
  const { view } = await mountPage(onTestFinished)
  await expect
    .poll(() => view.container.querySelector('[data-slot="logs-virtual-item"]'))
    .not.toBeNull()
  const panel = view.container.querySelector('[data-slot="core-logs"]')!
  expect(panel.querySelector('[role="status"]')).toBeNull()
  expect(
    [...panel.childNodes]
      .filter((node) => node.nodeType === Node.TEXT_NODE)
      .map((node) => node.textContent)
      .join(''),
  ).toBe('')
})

test('Core upward history preserves the visible row and follow-latest returns to the tail', async ({
  onTestFinished,
}) => {
  let finishOlder!: () => void
  const { view, queryCalls } = await mountPage(onTestFinished, {
    query_logs: (params) => {
      const { request } = params as { request: QueryLogs }
      if (request.direction === 'before')
        return new Promise((resolve) => {
          finishOlder = () => resolve(page(rows(181, 20), 181, false))
        })
      return request.direction === 'latest' ? page() : page([], 221, false)
    },
  })
  await expect
    .poll(() =>
      view.container.querySelector('[data-slot="scroll-area-viewport"]'),
    )
    .not.toBeNull()
  const viewport = view.container.querySelector<HTMLDivElement>(
    '[data-slot="scroll-area-viewport"]',
  )!
  await expect
    .poll(() => viewport.scrollTop, { timeout: 5_000 })
    .toBeGreaterThan(1000)
  await expect
    .poll(() => viewport.parentElement?.getAttribute('data-scroll-direction'))
    .toBe('down')
  expect(queryCalls().some((request) => request.direction === 'before')).toBe(
    false,
  )
  viewport.scrollTop = 0
  await expect
    .poll(
      () =>
        queryCalls().filter((request) => request.direction === 'before').length,
    )
    .toBe(1)
  const rowTop = () =>
    view
      .getByText('record 201', { exact: true })
      .element()
      .getBoundingClientRect().top - viewport.getBoundingClientRect().top
  const top = rowTop()
  finishOlder()
  await expect
    .poll(() => viewport.scrollTop, { timeout: 5_000 })
    .toBeGreaterThan(1000)
  await expect
    .poll(() => Math.abs(rowTop() - top), { timeout: 5_000 })
    .toBeLessThan(2)
  expect(view.container.querySelector('button')?.textContent).not.toBe(
    m.logs_load_older(),
  )
  viewport.scrollTop = 0
  await expect
    .element(view.getByText('record 181', { exact: true }))
    .toBeVisible()
  expect(
    queryCalls().filter((request) => request.direction === 'before'),
  ).toHaveLength(1)
  await view.getByRole('button', { name: m.logs_follow_latest() }).click()
  await expect
    .poll(() => viewport.scrollTop, { timeout: 5_000 })
    .toBeGreaterThan(1000)
})

test('empty filtered pages keep loading until history fills the viewport or ends', async ({
  onTestFinished,
}) => {
  const { view, queryCalls } = await mountPage(onTestFinished, {
    query_logs: (params) => {
      const { request } = params as { request: QueryLogs }
      const lastPage =
        request.direction === 'before' && request.cursor?.offset === '180'
      return page(
        lastPage ? rows(160, 2) : [],
        request.direction === 'latest' ? 200 : 180,
        !lastPage,
      )
    },
  })
  await expect
    .element(view.getByText('record 160', { exact: true }))
    .toBeVisible()
  expect(
    queryCalls()
      .filter((request) => request.direction === 'before')
      .map((request) => request.cursor?.offset),
  ).toEqual(['200', '180'])
})

test('actual Core host changes close the original source and reset archived selection', async ({
  onTestFinished,
}) => {
  let host: 'local' | 'service' = 'local'
  const { view, client, calls } = await mountPage(onTestFinished, {
    get_core_status: () => status(host),
    open_log_session: (params) => ({
      id: `${params?.source}:${(params?.request as { file: string | null }).file ?? 'current'}`,
      lease_ms: 60_000,
    }),
  })
  await expect.poll(() => calls('open_log_session').length).toBe(1)
  await view.getByRole('combobox', { name: m.logs_file_label() }).click()
  await view.getByRole('option', { name: 'core-000001.jsonl' }).click()
  await expect.poll(() => calls('open_log_session').length).toBe(2)
  host = 'service'
  await client.invalidateQueries({ queryKey: ['getCoreStatus'] })
  await expect
    .poll(() => calls('close_log_session'))
    .toContainEqual({
      source: 'core_local',
      session: 'core_local:core-000001.jsonl',
    })
  await expect
    .poll(() => calls('open_log_session').at(-1))
    .toMatchObject({ source: 'core_service', request: { file: null } })
  expect(
    view.getByRole('combobox', { name: m.logs_file_label() }).element()
      .textContent,
  ).toContain(m.logs_current_file())
  expect(
    view.getByRole('radiogroup', { name: m.logs_source_label() }).element()
      .textContent,
  ).toContain(m.logs_source_core())
})

test('a late open result is closed against its original Core source', async ({
  onTestFinished,
}) => {
  let host: 'local' | 'service' = 'local'
  let finishOpen!: (value: unknown) => void
  const { client, calls } = await mountPage(onTestFinished, {
    get_core_status: () => status(host),
    open_log_session: (params) =>
      params?.source === 'core_local'
        ? new Promise((resolve) => {
            finishOpen = resolve
          })
        : { id: 'service-session', lease_ms: 60_000 },
  })
  await expect.poll(() => calls('open_log_session').length).toBe(1)
  host = 'service'
  await client.invalidateQueries({ queryKey: ['getCoreStatus'] })
  await expect.poll(() => calls('open_log_session').length).toBe(2)
  finishOpen({ id: 'late-local', lease_ms: 60_000 })
  await expect
    .poll(() => calls('close_log_session'))
    .toContainEqual({ source: 'core_local', session: 'late-local' })
  expect(
    calls('query_logs').every((params) => params.source === 'core_service'),
  ).toBe(true)
})

test('unsupported service Core archives show an error without falling back to local files', async ({
  onTestFinished,
}) => {
  const { view, calls } = await mountPage(onTestFinished, {
    get_core_status: () => status('service'),
    list_log_files: () => logFailure('unsupported'),
  })
  await expect
    .element(view.getByText(m.logs_service_unsupported()))
    .toBeVisible()
  expect(calls('list_log_files')).toEqual([{ source: 'core_service' }])
  expect(calls('open_log_session')).toHaveLength(0)
})

test('unavailable Core status does not guess a local source', async ({
  onTestFinished,
}) => {
  const { view, calls } = await mountPage(onTestFinished, {
    get_core_status: () => Promise.reject(new Error('status unavailable')),
  })
  await expect
    .element(view.getByText(m.logs_source_unavailable()))
    .toBeVisible()
  expect(calls('list_log_files')).toHaveLength(0)
})

test('Core clear only hides displayed rows and reopening the file restores the archive', async ({
  onTestFinished,
}) => {
  const { view, calls } = await mountPage(onTestFinished)
  const clear = view.getByRole('button', { name: m.logs_clear_display() })
  await expect.element(clear).toBeEnabled()
  await clear.click()
  await expect.element(view.getByText(m.logs_empty_message())).toBeVisible()
  expect(calls('open_log_session')).toHaveLength(1)
  expect(calls('close_log_session')).toHaveLength(0)
  window.dispatchEvent(new Event('focus'))
  await expect
    .poll(() => calls('query_logs').at(-1))
    .toMatchObject({ request: { direction: 'after', cursor: cursor(221) } })
  await view.getByRole('combobox', { name: m.logs_file_label() }).click()
  await view.getByRole('option', { name: 'core-000001.jsonl' }).click()
  await expect
    .poll(
      () =>
        view.container.querySelectorAll('[data-slot="logs-virtual-item"]')
          .length,
    )
    .toBeGreaterThan(0)
})

test('current Core file follows rotation and replaces the old generation', async ({
  onTestFinished,
}) => {
  let rotated = false
  const { view, calls } = await mountPage(onTestFinished, {
    list_log_files: () => [
      {
        id: rotated ? 'core-000003.jsonl' : 'core-000002.jsonl',
        name: rotated ? 'core-000003.jsonl' : 'core-000002.jsonl',
        bytes: '100',
      },
    ],
    query_logs: (params) => {
      const { request } = params as { request: QueryLogs }
      if (rotated && request.cursor?.generation === 'archive')
        return logFailure('cursor_reset')
      if (rotated)
        return {
          ...page(rows(300, 2, 'rotated'), 300, false, 'rotated'),
          file: 'core-000003.jsonl',
        }
      return page(rows(1, 2), 1, false)
    },
  })
  await expect
    .element(view.getByText('record 2', { exact: true }))
    .toBeVisible()
  rotated = true
  window.dispatchEvent(new Event('focus'))
  await expect
    .element(view.getByText('record 301', { exact: true }))
    .toBeVisible()
  expect(view.container.textContent).not.toContain('record 2')
  expect(calls('open_log_session')).toHaveLength(1)
  expect(calls('open_log_session')[0]).toMatchObject({
    request: { file: null },
  })
  await view.getByRole('combobox', { name: m.logs_file_label() }).click()
  await expect
    .element(view.getByRole('option', { name: 'core-000003.jsonl' }))
    .toBeVisible()
})

test('Core text filters use archive queries and JSON/copy preserve the full frame', async ({
  onTestFinished,
}) => {
  const record = rows(1, 1)[0]
  const writeText = vi
    .spyOn(navigator.clipboard, 'writeText')
    .mockResolvedValue(undefined)
  onTestFinished(() => writeText.mockRestore())
  const { view, calls } = await mountPage(onTestFinished, {
    query_logs: () => page([record], 1, false),
  })
  await expect
    .element(view.getByText('record 1', { exact: true }))
    .toBeVisible()
  await view.getByRole('button', { name: m.logs_copy() }).click()
  expect(writeText).toHaveBeenCalledWith(record.raw)
  await view.getByRole('button', { name: m.logs_view_json() }).click()
  await expect
    .poll(() => view.container.querySelector('[role="region"]')?.textContent)
    .toContain('"epoch": 9')
  const json = view.container.querySelector('[role="region"]')!.textContent!
  for (const key of ['at', 'kind', 'stream', 'raw', 'truncated'])
    expect(json).toContain(`"${key}"`)
  await view
    .getByRole('searchbox', { name: m.logs_filter_placeholder() })
    .fill('record')
  await expect
    .poll(() => calls('query_logs').at(-1))
    .toMatchObject({
      source: 'core_local',
      request: { filter: { text: 'record' } },
    })
})

test('a retained Core archive removed by retention reports file loss', async ({
  onTestFinished,
}) => {
  const { view, calls } = await mountPage(onTestFinished, {
    open_log_session: (params) =>
      (params?.request as { file: string | null }).file
        ? logFailure('file_gone')
        : { id: 'current', lease_ms: 60_000 },
  })
  await expect.poll(() => calls('open_log_session').length).toBe(1)
  await view.getByRole('combobox', { name: m.logs_file_label() }).click()
  await view.getByRole('option', { name: 'core-000001.jsonl' }).click()
  await expect.element(view.getByText(m.logs_file_gone())).toBeVisible()
  expect(calls('close_log_session')).toContainEqual({
    source: 'core_local',
    session: 'current',
  })
  expect(calls('open_log_session').at(-1)).toMatchObject({
    source: 'core_local',
    request: { file: 'core-000001.jsonl' },
  })
})
