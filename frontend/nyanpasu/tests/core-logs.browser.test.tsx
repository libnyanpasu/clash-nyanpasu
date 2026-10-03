import type { ComponentType, PropsWithChildren } from 'react'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import type {
  CoreLogQuery,
  CoreLogRow,
  CoreLogStatus,
} from '@nyanpasu/rpc/types'
import { Route } from '../src/pages/(main)/main/logs'
import { m } from '../src/paraglide/messages'

const mocked = vi.hoisted(() => ({
  queryCoreLogs: vi.fn(),
  getCoreLogStatus: vi.fn(),
  navigate: vi.fn(),
  source: 'core' as 'core' | 'app' | 'service',
  listLogFiles: vi.fn(),
  openLogSession: vi.fn(),
  closeLogSession: vi.fn(),
  queryLogs: vi.fn(),
}))

vi.mock('@nyanpasu/query/provider/rpc-provider', async (importOriginal) => ({
  ...(await importOriginal<
    typeof import('@nyanpasu/query/provider/rpc-provider')
  >()),
  useRpc: () => rpc,
  useQueryApi: () => rpc,
}))

vi.mock('../src/pages/(main)/main/logs/route', () => ({
  Route: {
    useSearch: () => ({ source: mocked.source }),
    useNavigate: () => mocked.navigate,
  },
  LogLevelsLayout: ({ children }: PropsWithChildren) => children,
}))

vi.mock('../src/components/providers/context-menu-provider', () => ({
  RegisterContextMenu: ({ children }: PropsWithChildren) => children,
  RegisterContextMenuTrigger: ({ children }: PropsWithChildren) => children,
  RegisterContextMenuContent: () => null,
}))

const rpc = {
  ...mocked,
  events: { coreLogsChanged: { listen: async () => () => {} } },
  listenResync: () => () => {},
}
const CoreLogsPage = Route.options.component as ComponentType
const ok = <T,>(data: T) => ({ status: 'ok' as const, data })
const cursor = (sequence: number) => ({ generation: 'session', sequence })
const status: CoreLogStatus = {
  generation: 'session',
  version: 220,
  first: cursor(1),
  head: cursor(220),
  bytes: 22000,
  discarded: 0,
  error: null,
}
const rows = (start: number, count: number): CoreLogRow[] =>
  Array.from({ length: count }, (_, index) => ({
    id: cursor(start + index),
    truncated: false,
    record: {
      source: {
        capture: 'capture',
        instance_id: 'instance',
        core_kind: 'Mihomo',
      },
      received_at: start + index,
      time: null,
      type: 'info',
      payload: `record ${start + index}`,
    },
  }))

beforeEach(() => {
  vi.clearAllMocks()
  mocked.source = 'core'
  mocked.getCoreLogStatus.mockResolvedValue(ok(status))
  mocked.queryCoreLogs.mockImplementation(async (query: CoreLogQuery) =>
    ok({
      rows: query.direction === 'latest' ? rows(201, 20) : [],
      cursor: cursor(201),
      more: query.direction === 'latest',
      status,
    }),
  )
})

test.for(['app', 'service'] as const)(
  '%s footer selects current and archived files through the MDY select',
  async (source, { onTestFinished }) => {
    mocked.source = source
    mocked.listLogFiles.mockResolvedValue(
      ok([{ id: 'archive.log', name: 'archive.log', bytes: '100' }]),
    )
    mocked.openLogSession.mockResolvedValue(
      ok({ id: 'session', lease_ms: 60000 }),
    )
    mocked.closeLogSession.mockResolvedValue(ok(null))
    mocked.queryLogs.mockResolvedValue(
      ok({
        rows: [],
        cursor: null,
        head: null,
        start: '0',
        file: 'archive.log',
        building: false,
        more: false,
        partial: false,
        malformed: '0',
        truncated: '0',
        indexed_bytes: '100',
        file_bytes: '100',
      }),
    )
    const view = await render(<CoreLogsPage />)
    onTestFinished(() => view.unmount())
    await expect.poll(() => mocked.openLogSession.mock.calls.length).toBe(1)
    const select = view.getByRole('combobox', { name: m.logs_file_label() })
    expect(select.element().textContent).toContain(m.logs_current_file())
    await select.click()
    await view.getByRole('option', { name: 'archive.log' }).click()
    await expect
      .poll(() => mocked.openLogSession.mock.lastCall)
      .toEqual([
        source,
        { request_id: expect.any(String), file: 'archive.log' },
      ])
    expect(select.element().textContent).toContain('archive.log')
    await select.click()
    await view.getByRole('option', { name: m.logs_current_file() }).click()
    await expect
      .poll(() => mocked.openLogSession.mock.lastCall)
      .toEqual([source, { request_id: expect.any(String), file: null }])
  },
)

async function mountPage() {
  const view = await render(
    <>
      <style>{`
        [data-slot="scroll-area"] { height: 330px; overflow: hidden; }
        [data-slot="scroll-area-viewport"] { height: 330px; }
        [data-slot="logs-virtual-list"] { position: relative; }
        [data-slot="logs-virtual-item"] { position: absolute; top: 0; width: 100%; height: 110px; }
      `}</style>
      <CoreLogsPage />
    </>,
  )
  const viewport = view.container.querySelector<HTMLDivElement>(
    '[data-slot="scroll-area-viewport"]',
  )!
  return { view, viewport }
}

test('zero discarded logs do not render a stray header or text node', async ({
  onTestFinished,
}) => {
  const { view } = await mountPage()
  onTestFinished(() => view.unmount())
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

test('scrolling to the top loads older records and preserves the visible row', async ({
  onTestFinished,
}) => {
  let finishOlder!: () => void
  mocked.queryCoreLogs.mockImplementation((query: CoreLogQuery) => {
    if (query.direction === 'before') {
      return new Promise((resolve) => {
        finishOlder = () =>
          resolve(
            ok({
              rows: rows(181, 20),
              cursor: cursor(181),
              more: false,
              status,
            }),
          )
      })
    }
    return Promise.resolve(
      ok({
        rows: query.direction === 'latest' ? rows(201, 20) : [],
        cursor: cursor(201),
        more: query.direction === 'latest',
        status,
      }),
    )
  })
  const { view, viewport } = await mountPage()
  onTestFinished(() => view.unmount())
  await expect
    .poll(() => viewport.scrollTop, { timeout: 5_000 })
    .toBeGreaterThan(1000)
  await expect
    .poll(() => viewport.parentElement?.getAttribute('data-scroll-direction'))
    .toBe('down')
  expect(
    mocked.queryCoreLogs.mock.calls.some(
      ([query]) => query.direction === 'before',
    ),
  ).toBe(false)
  viewport.scrollTop = 0
  await expect
    .poll(
      () =>
        mocked.queryCoreLogs.mock.calls.filter(
          ([query]) => query.direction === 'before',
        ).length,
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
    mocked.queryCoreLogs.mock.calls.filter(
      ([query]) => query.direction === 'before',
    ),
  ).toHaveLength(1)
  await view.getByRole('button', { name: m.logs_follow_latest() }).click()
  await expect
    .poll(() => viewport.scrollTop, { timeout: 5_000 })
    .toBeGreaterThan(1000)
})

test('empty filtered pages continue loading until history fills the viewport or ends', async ({
  onTestFinished,
}) => {
  mocked.queryCoreLogs.mockImplementation(async (query: CoreLogQuery) => {
    const lastPage =
      query.direction === 'before' && query.cursor?.sequence === 180
    return ok({
      rows: lastPage ? rows(160, 2) : [],
      cursor: cursor(query.direction === 'latest' ? 200 : 180),
      more: !lastPage,
      status,
    })
  })
  const { view } = await mountPage()
  onTestFinished(() => view.unmount())
  await expect
    .element(view.getByText('record 160', { exact: true }))
    .toBeVisible()
  expect(
    mocked.queryCoreLogs.mock.calls
      .filter(([query]) => query.direction === 'before')
      .map(([query]) => query.cursor.sequence),
  ).toEqual([200, 180])
})
