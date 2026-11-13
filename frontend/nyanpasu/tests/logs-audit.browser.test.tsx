import type { PropsWithChildren } from 'react'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import type {
  AuditCursor,
  AuditFilter,
  AuditPage,
  AuditRow,
} from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route } from '../src/pages/(main)/main/logs/audit/index'
import { m } from '../src/paraglide/messages'

const mocked = vi.hoisted(() => ({
  queryRpcAudit:
    vi.fn<
      (
        filter: AuditFilter,
        before: AuditCursor | null,
        limit: number,
      ) => Promise<unknown>
    >(),
  navigate: vi.fn(),
}))

vi.mock('@nyanpasu/query/provider/rpc-provider', async (importOriginal) => ({
  ...(await importOriginal<
    typeof import('@nyanpasu/query/provider/rpc-provider')
  >()),
  useQueryApi: () => mocked,
}))

vi.mock('../src/pages/(main)/main/logs/route', () => ({
  Route: { useNavigate: () => mocked.navigate },
  LogLevelsLayout: ({ children }: PropsWithChildren) => children,
}))

const ok = <T,>(data: T) => ({ status: 'ok' as const, data })

const cursor = (seq: number): AuditCursor => ({
  epoch: 1,
  key: seq,
  incarnation: 1,
  seq,
})

const row = (method: string, at: number): AuditRow => ({
  at,
  transport: 'local',
  method,
  owner: 'owner',
  invocation_id: at,
  request_id: null,
  phase: 'finished',
  outcome: 'success',
  kind: null,
  code: null,
  duration_ns: 1_500_000,
})

const page = (
  rows: AuditRow[],
  next: AuditCursor | null = null,
  dropped = 0,
): AuditPage => ({ rows, next, dropped })

const rootRoute = createRootRoute({ component: Outlet })
// The test router has no logs layout above the page.
Route.update({
  id: '/(main)/main/logs/audit/',
  path: '/main/logs/audit/',
  getParentRoute: () => rootRoute,
} as never)

async function renderPage(url = '/main/logs/audit/') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([Route]),
    history: createMemoryHistory({ initialEntries: [url] }),
  })
  const view = await render(
    <QueryClientProvider client={client}>
      <style>{`[data-slot="scroll-area"] { height: 400px; }`}</style>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
  return { view, client }
}

const requests = () =>
  mocked.queryRpcAudit.mock.calls.map(([filter, before, limit]) => ({
    filter,
    before,
    limit,
  }))

beforeEach(() => {
  vi.clearAllMocks()
  mocked.queryRpcAudit.mockResolvedValue(ok(page([row('get_profiles', 1)])))
})

test('the filters in the URL and sidebar become the request filter', async ({
  onTestFinished,
}) => {
  const { view, client } = await renderPage(
    '/main/logs/audit/?transport=local&outcome=failure',
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  await expect.poll(requests).toEqual([
    {
      filter: { transport: 'local', phase: null, outcome: 'failure' },
      before: null,
      limit: 200,
    },
  ])

  await view.getByRole('link', { name: m.logs_audit_transport_http() }).click()
  await expect
    .poll(() => requests().at(-1)?.filter)
    .toEqual({
      transport: 'http',
      phase: null,
      outcome: 'failure',
    })

  await view
    .getByRole('link', { name: m.logs_audit_outcome_rejected() })
    .click()
  await expect
    .poll(() => requests().at(-1)?.filter)
    .toEqual({
      transport: 'http',
      phase: 'rejected',
      outcome: null,
    })

  await view.getByRole('link', { name: m.logs_audit_all_outcomes() }).click()
  await expect
    .poll(() => requests().at(-1)?.filter)
    .toEqual({
      transport: 'http',
      phase: null,
      outcome: null,
    })
})

test('load more reads older records and refresh starts over from the newest', async ({
  onTestFinished,
}) => {
  mocked.queryRpcAudit.mockImplementation(
    async (_filter: AuditFilter, before: AuditCursor | null) =>
      ok(
        before
          ? page([row('older_call', 1)])
          : page([row('newest_call', 2)], cursor(2)),
      ),
  )
  const { view, client } = await renderPage()
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  await expect
    .element(view.getByText('newest_call', { exact: true }))
    .toBeInTheDocument()
  await view.getByRole('button', { name: m.logs_load_older() }).click()
  await expect
    .element(view.getByText('older_call', { exact: true }))
    .toBeInTheDocument()
  expect(requests().at(-1)?.before).toEqual(cursor(2))
  await expect
    .element(view.getByRole('button', { name: m.logs_load_older() }))
    .not.toBeInTheDocument()

  await view.getByRole('button', { name: m.logs_audit_refresh() }).click()
  await expect.poll(() => requests().length).toBe(3)
  expect(requests().at(-1)?.before).toBeNull()
  await expect
    .element(view.getByText('older_call', { exact: true }))
    .not.toBeInTheDocument()
  await expect
    .element(view.getByText('newest_call', { exact: true }))
    .toBeInTheDocument()
})

test('a warning strip appears only when records were dropped', async ({
  onTestFinished,
}) => {
  const first = await renderPage()
  await expect
    .element(first.view.getByText('get_profiles', { exact: true }))
    .toBeInTheDocument()
  expect(first.view.getByRole('alert').query()).toBeNull()
  await first.view.unmount()
  first.client.clear()

  mocked.queryRpcAudit.mockResolvedValue(
    ok(page([row('get_profiles', 1)], null, 3)),
  )
  const { view, client } = await renderPage()
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.logs_audit_dropped({ count: 3 }))
})

test('the source tabs navigate between the audit page and the log sources', async ({
  onTestFinished,
}) => {
  const { view, client } = await renderPage()
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  const tabs = view.getByRole('radiogroup', { name: m.logs_source_label() })
  await expect
    .element(tabs.getByRole('radio', { name: m.logs_source_audit() }))
    .toBeChecked()
  await tabs.getByRole('radio', { name: m.logs_source_core() }).click()
  expect(mocked.navigate).toHaveBeenLastCalledWith({
    to: '/main/logs',
    search: expect.any(Function),
  })
  const { search } = mocked.navigate.mock.lastCall![0] as {
    search: (previous: object) => object
  }
  expect(search({ level: 'error', transport: 'http' })).toEqual({
    level: 'error',
    source: 'core',
  })
})
