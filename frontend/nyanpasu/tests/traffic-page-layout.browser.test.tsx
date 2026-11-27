import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { dimensionName, usageLabel } from '@/utils/traffic-usage'
import type {
  Dimension,
  MeowTrafficLedger,
  MeowTrafficLedgerQuery,
  ReportRequest,
  Topology,
  TrafficReport,
  UsageGroup,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type AnyRoute,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import {
  DEFAULT_TEST_CAPABILITIES,
  TestQueryProvider as QueryClientProvider,
} from './query-provider'
import '../src/utils/language'
import { Route as overviewRoute } from '../src/pages/(main)/main/topology'
import type { TrafficSearch } from '../src/pages/(main)/main/topology/_modules/search'
import { TRAFFIC_VIEW_KV_KEY } from '../src/pages/(main)/main/topology/_modules/view-settings'
import { Route as rankingsRoute } from '../src/pages/(main)/main/topology/$dimension'
import { Route as flowRoute } from '../src/pages/(main)/main/topology/flow'
import { Route as mapRoute } from '../src/pages/(main)/main/topology/map'
import { Route as meowRoute } from '../src/pages/(main)/main/topology/meow'
import { Route as trafficRoute } from '../src/pages/(main)/main/topology/route'

vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

const backend = vi.hoisted(() => ({
  intervals: [] as unknown[],
  retention: '7d',
  usage: [] as UsageGroup[],
  core: 'mihomo',
  ledgerQueries: [] as MeowTrafficLedgerQuery[],
  ledger: null as MeowTrafficLedger | null,
}))

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const original = await importOriginal<typeof import('@nyanpasu/query')>()

  return {
    ...original,
    // Traffic reports use RPC, not live connection-detail frames. Keep the real
    // animated outlet while leaving that independent stream boundary out here.
    ClashConnectionDetailsFreezeBoundary: ({
      children,
    }: {
      children: ReactNode
    }) => children,
    useTrafficReport: (
      ...args: Parameters<typeof original.useTrafficReport>
    ) => {
      backend.intervals.push(args[1]?.refetchInterval)
      return original.useTrafficReport(...args)
    },
    useSetting: (key: string) => ({
      value: key === 'core' ? backend.core : backend.retention,
    }),
    useProfile: () => ({
      query: { data: { items: [{ uid: 'main', name: 'Main profile' }] } },
    }),
  }
})

const usage = (upload: number, download: number, connections: number) => ({
  bytes: { upload, download },
  connections,
})

const group = (key: string, bytes = 1024): UsageGroup => ({
  key,
  usage: usage(bytes, bytes * 2, 3),
  current_rate: null,
})

const ranking = (
  dimension: TrafficReport['rankings'][number]['dimension'],
  groups: UsageGroup[],
  distinct = groups.length,
): TrafficReport['rankings'][number] => ({
  dimension,
  distinct,
  groups,
  other: usage(512, 512, 1),
})

const report: TrafficReport = {
  total: usage(1024, 4096, 12),
  current_rate: null,
  rankings: [
    // Three rankings with others and two of a single value.
    ranking('source', [group('192.168.1.2')], 4),
    ranking('inbound', [group('mixed')]),
    ranking('target', [group('example.com')], 9),
    ranking('exit', [group('Node-A')]),
    ranking('process', [group('C:/Tools/curl.exe')], 6),
  ],
  topology: null,
}

const rpc = vi.hoisted(() => ({
  reports: [] as ReportRequest[],
  // The KV store, by key.
  storage: new Map<string, string>(),
  storageRevision: 0,
}))

type RpcCall = { method: string; params: Record<string, unknown> }

function topologyFor(layers: readonly Dimension[]): Topology {
  const nodes = layers.map((dimension, layer) => {
    const key =
      dimension === 'source_region'
        ? 'CN'
        : dimension === 'destination_region'
          ? 'US'
          : dimension === 'destination_basis'
            ? 'dialed'
            : `${dimension}-1`

    return {
      id: JSON.stringify([layer, key]),
      layer,
      key,
      usage: usage(1024, 2048, 3),
    }
  })
  const edges = nodes.slice(0, -1).map((source, index) => ({
    source: source.id,
    target: nodes[index + 1].id,
    usage: usage(512, 512, 1),
  }))

  return { nodes, edges }
}

function mockBackend() {
  mockIPC(async (command, args) => {
    if (command !== 'call_rpc') return

    const { method, params } = args as RpcCall

    if (method === 'query_meow_traffic_ledger') {
      backend.ledgerQueries.push(params.query as MeowTrafficLedgerQuery)
      return backend.ledger
    }

    if (method === 'get_meow_statistics') {
      return {
        uploadTotal: 1_024,
        downloadTotal: 8_192,
        proxies: {
          Exit: { upload: 1_024, download: 8_192 },
          Relay: { upload: 1_024, download: 8_192 },
          Unused: { upload: 0, download: 0 },
        },
      }
    }

    if (method === 'query_traffic_report') {
      const request = params.request as ReportRequest
      rpc.reports.push(request)

      return {
        ...report,
        topology: request.topology
          ? topologyFor(request.topology.layers)
          : null,
      }
    }

    if (method === 'query_traffic_usage') {
      return {
        total: report.total,
        groups: backend.usage.length
          ? backend.usage
          : (report.rankings.find(
              ({ dimension }) => dimension === params.groupBy,
            )?.groups ?? []),
        other: usage(0, 0, 0),
        next: null,
      }
    }

    if (method === 'get_storage_item') {
      return {
        value: rpc.storage.get(params.key as string) ?? null,
        revision: rpc.storageRevision,
      }
    }

    if (method === 'set_storage_item') {
      rpc.storage.set(params.key as string, params.value as string)
      rpc.storageRevision += 1
      return rpc.storageRevision
    }

    throw new Error(`unexpected rpc ${method}`)
  })
}

function initialEntry(initial?: Partial<TrafficSearch>) {
  const params = new URLSearchParams()

  for (const [key, value] of Object.entries(initial ?? {})) {
    if (value === undefined || (key === 'view' && value === 'overview'))
      continue

    params.set(key, typeof value === 'string' ? value : JSON.stringify(value))
  }

  const query = params.toString()
  return `/main/topology${query ? `?${query}` : ''}`
}

function createTestRoute(
  fileRoute: { options: object },
  id: string,
  path: string,
  parentRoute: AnyRoute,
) {
  const getParentRoute = () => parentRoute
  const route = createRoute({
    ...fileRoute.options,
    path,
    getParentRoute,
  } as never)

  return route.update({ id, path, getParentRoute } as never)
}

async function open(
  onFinished: (fn: () => void | Promise<void>) => void,
  initial?: Partial<TrafficSearch>,
  allowMeow = false,
) {
  const rootRoute = createRootRoute({
    component: () => (
      <div className="flex h-dvh w-dvw">
        <Outlet />
      </div>
    ),
  })
  const topologyRoute = createTestRoute(
    trafficRoute,
    '/main/topology',
    '/main/topology',
    rootRoute,
  )
  const routeTree = rootRoute.addChildren([
    topologyRoute.addChildren([
      createTestRoute(overviewRoute, '/', '/', topologyRoute),
      createTestRoute(flowRoute, '/flow/', '/flow/', topologyRoute),
      createTestRoute(mapRoute, '/map/', '/map/', topologyRoute),
      createTestRoute(meowRoute, '/meow/', '/meow/', topologyRoute),
      createTestRoute(
        rankingsRoute,
        '/$dimension/',
        '/$dimension/',
        topologyRoute,
      ),
    ] as never),
  ] as never)
  const router = createRouter({
    routeTree: routeTree as unknown as AnyRoute,
    history: createMemoryHistory({ initialEntries: [initialEntry(initial)] }),
    context: {},
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider
      client={queries}
      capabilities={{
        ...DEFAULT_TEST_CAPABILITIES,
        commands: allowMeow
          ? [
              {
                name: 'get_app_settings',
                permission: { resource: 'settings', access: 'read' },
                effect: 'query',
                sensitive: false,
                allowed: true,
              },
              ...['get_meow_statistics', 'query_meow_traffic_ledger'].map(
                (name) => ({
                  name,
                  permission: {
                    resource: 'traffic' as const,
                    access: 'read' as const,
                  },
                  effect: 'query' as const,
                  sensitive: true,
                  allowed: true,
                }),
              ),
            ]
          : [],
      }}
    >
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )

  onFinished(async () => {
    await view.unmount()
    queries.clear()
    clearMocks()
    // The KV store caches what it reads.
    localStorage.removeItem(`nyanpasu-kv-:${btoa(TRAFFIC_VIEW_KV_KEY)}`)
    rpc.storage.clear()
    rpc.reports = []
    backend.intervals = []
    backend.retention = '7d'
    backend.usage = []
    backend.core = 'mihomo'
    backend.ledgerQueries = []
    backend.ledger = null
  })

  return view
}

const sidebarItem = (view: Awaited<ReturnType<typeof open>>, name: string) =>
  view
    .getByRole('complementary', { name: m.traffic_sidebar() })
    .getByRole('link', { name })

test('Meow traffic filters the persisted ledger and uses the session totals from the core', async ({
  onTestFinished,
}) => {
  // Meow records local days; noon UTC keeps the local day 2026-01-01 in every
  // zone from UTC-12 to UTC+11, so the local day is stable across CI runners.
  vi.setSystemTime(new Date('2026-01-01T12:00:00Z'))
  onTestFinished(() => {
    vi.useRealTimers()
  })
  backend.core = 'meow-alpha'
  backend.ledger = {
    enabled: true,
    upload: 1_024,
    download: 4_096,
    requests: 3,
    rows: [
      {
        name: 'example.com',
        upload: 1_024,
        download: 4_096,
        requests: 3,
        via_upload: 0,
        via_download: 0,
      },
      ...Array.from({ length: 50 }, (_, index) => ({
        name: `host-${index}.test`,
        upload: 0,
        download: 0,
        requests: 0,
      })),
    ],
  }
  mockBackend()
  const view = await open(onTestFinished, undefined, true)
  await sidebarItem(view, m.meow_traffic_title()).click()
  await expect
    .element(view.getByRole('table', { name: m.traffic_dimension_target() }))
    .toBeVisible()
  expect(backend.ledgerQueries.at(-1)).toMatchObject({
    from: '2026-01-01',
    to: '2026-01-01',
  })
  const reads = backend.ledgerQueries.length
  await view.getByRole('button', { name: m.inspect_refresh() }).click()
  await expect.poll(() => backend.ledgerQueries.length).toBeGreaterThan(reads)
  await expect.poll(() => view.getByRole('row').elements().length).toBe(51)
  await view.getByRole('button', { name: m.meow_traffic_next_page() }).click()
  await expect
    .element(view.getByRole('button', { name: 'host-49.test' }))
    .toBeVisible()
  await expect.poll(() => view.getByRole('row').elements().length).toBe(2)
  await view.getByRole('combobox', { name: m.traffic_range_label() }).click()
  await view.getByRole('option', { name: m.meow_traffic_last_month() }).click()
  await expect
    .poll(() => backend.ledgerQueries.at(-1))
    .toMatchObject({ from: '2025-12-01', to: '2025-12-31' })
  await view.getByRole('button', { name: 'example.com' }).click()
  await expect
    .poll(() => backend.ledgerQueries.at(-1))
    .toMatchObject({
      host: 'example.com',
      by: 'app',
    })
  await view
    .getByRole('button', {
      name: m.traffic_filter_remove({
        filter: `${m.traffic_dimension_target()} · example.com`,
      }),
    })
    .click()
  await expect
    .poll(() => backend.ledgerQueries.at(-1))
    .toMatchObject({
      host: null,
      by: 'host',
    })

  await view.getByText(m.meow_traffic_statistics(), { exact: true }).click()
  const policyTable = view.getByRole('table', { name: m.meow_traffic_policy() })
  await expect
    .element(policyTable.getByRole('rowheader', { name: 'Relay' }))
    .toBeVisible()
  await expect
    .element(policyTable.getByRole('rowheader', { name: 'Unused' }))
    .not.toBeInTheDocument()
  await expect
    .element(view.getByRole('definition').first())
    .toHaveTextContent('9.00 KiB')
})

test('an older Meow core can lack the traffic ledger and still offer statistics', async ({
  onTestFinished,
}) => {
  backend.core = 'meow'
  mockBackend()
  const view = await open(onTestFinished, undefined, true)
  await sidebarItem(view, m.meow_traffic_title()).click()
  await expect
    .element(view.getByText(m.meow_traffic_ledger_unsupported()))
    .toBeVisible()
  await view.getByText(m.meow_traffic_statistics(), { exact: true }).click()
  await expect
    .element(view.getByRole('table', { name: m.meow_traffic_policy() }))
    .toBeVisible()
})

test('the full listing of a ranking mounts only the rows in view', async ({
  onTestFinished,
}) => {
  mockBackend()
  backend.usage = Array.from({ length: 300 }, (_, index) =>
    group(`host-${index}.example.com`, 300 - index),
  )
  const view = await open(onTestFinished)
  await view
    .getByRole('link', { name: m.traffic_rank_view_all() })
    .first()
    .click()
  await expect
    .element(view.getByRole('button', { name: /host-0\.example\.com/ }))
    .toBeInTheDocument()

  const rows = document.querySelector<HTMLElement>(
    '[data-slot="traffic-usage-rows"]',
  )!
  expect(rows.querySelectorAll('button').length).toBeLessThan(40)

  const viewport = rows.closest<HTMLElement>(
    '[data-slot="scroll-area-viewport"]',
  )!
  await expect
    .poll(() => {
      viewport.scrollTop = viewport.scrollHeight
      return rows.textContent?.includes('host-299.example.com') ?? false
    })
    .toBe(true)
  const last = view.getByRole('button', { name: /host-299\.example\.com/ })
  await expect.element(last).toBeInTheDocument()
  expect(rows.querySelectorAll('button').length).toBeLessThan(40)
})

// The menu sits in a portal over the page, where a pointer click waits on
// actionability checks that never settle in the test frame.
const pick = async (locator: { element: () => Element }) =>
  (locator.element() as HTMLElement).click()

test('flow rankings settings persist and restore through the view settings menu', async ({
  onTestFinished,
}) => {
  rpc.storage.set(TRAFFIC_VIEW_KV_KEY, JSON.stringify({ rankings: [] }))
  mockBackend()
  const view = await open(onTestFinished, { view: 'flow' })

  const show = view.getByRole('button', {
    name: m.traffic_view_rankings_show(),
  })
  await expect.element(show).toBeInTheDocument()
  expect(
    view.getByRole('heading', { name: m.traffic_view_rankings() }).elements(),
  ).toHaveLength(0)

  await show.click()
  await pick(
    view.getByRole('menuitemcheckbox', { name: m.traffic_rank_process() }),
  )
  await expect
    .poll(
      () =>
        document.querySelector('[data-slot="traffic-view-rankings"] h2')
          ?.textContent,
    )
    .toBe(m.traffic_view_rankings())
  await expect
    .poll(() => JSON.parse(rpc.storage.get(TRAFFIC_VIEW_KV_KEY) ?? 'null'))
    .toEqual(expect.objectContaining({ rankings: ['process'] }))

  for (const title of [m.traffic_rank_source(), m.traffic_rank_inbound()]) {
    await pick(view.getByRole('menuitemcheckbox', { name: title }))
  }
  await expect
    .poll(
      () =>
        document.querySelectorAll(
          '[data-slot="traffic-view-rankings"] [data-slot="traffic-ranking"]',
        ).length,
    )
    .toBe(3)
  await expect
    .poll(() => JSON.parse(rpc.storage.get(TRAFFIC_VIEW_KV_KEY) ?? 'null'))
    .toEqual(
      expect.objectContaining({
        rankings: ['process', 'source', 'inbound'],
      }),
    )
})

test('every sidebar entry selects the matching page view', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  const entries = [
    ['overview', m.traffic_overview()],
    ['flow', m.topology_flow()],
    ['map', m.topology_geo_map()],
    ['process', dimensionName('process')],
  ] as const

  for (const [value, label] of entries) {
    const item = sidebarItem(view, label)
    await item.click()
    await expect
      .element(view.getByRole('heading', { name: label, level: 1 }))
      .toBeInTheDocument()
    await expect.element(item).toHaveAttribute('data-active', 'true')

    if (value === 'map') {
      await expect
        .poll(() => rpc.reports.at(-1)?.topology?.layers)
        .toEqual(['source_region', 'destination_region', 'destination_basis'])
    } else if (value === 'flow') {
      await expect
        .poll(() => rpc.reports.at(-1)?.topology?.layers)
        .toEqual(['origin', 'rule', 'chain', 'exit'])
    } else if (value === 'process') {
      // A dimension's entry opens its full listing.
      const ranking = report.rankings.find(
        (item) => item.dimension === 'process',
      )!
      const rowName = usageLabel('process', ranking.groups[0]!.key).text
      await expect
        .element(
          view.getByRole('button', {
            name: new RegExp(rowName.replaceAll('.', '\\.')),
          }),
        )
        .toBeInTheDocument()
    }
  }
})
