import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { dimensionName, usageLabel } from '@/utils/traffic-usage'
import type {
  Dimension,
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
import { TestQueryProvider as QueryClientProvider } from './query-provider'
import '../src/utils/language'
import { Route as overviewRoute } from '../src/pages/(main)/main/topology'
import type { TrafficSearch } from '../src/pages/(main)/main/topology/_modules/search'
import { Route as exitRoute } from '../src/pages/(main)/main/topology/exit'
import { Route as flowRoute } from '../src/pages/(main)/main/topology/flow'
import { Route as inboundRoute } from '../src/pages/(main)/main/topology/inbound'
import { Route as mapRoute } from '../src/pages/(main)/main/topology/map'
import { Route as processRoute } from '../src/pages/(main)/main/topology/process'
import { Route as trafficRoute } from '../src/pages/(main)/main/topology/route'
import { Route as sourceRoute } from '../src/pages/(main)/main/topology/source'
import { Route as targetRoute } from '../src/pages/(main)/main/topology/target'

vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

const backend = vi.hoisted(() => ({
  intervals: [] as unknown[],
  retention: '7d',
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
    useSetting: () => ({ value: backend.retention }),
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
): TrafficReport['rankings'][number] => ({
  dimension,
  distinct: groups.length,
  groups,
  other: usage(512, 512, 1),
})

const report: TrafficReport = {
  total: usage(1024, 4096, 12),
  current_rate: null,
  rankings: [
    ranking('source', [group('192.168.1.2')]),
    ranking('inbound', [group('mixed')]),
    ranking('target', [group('example.com')]),
    ranking('exit', [group('Node-A')]),
    ranking('process', [group('C:/Tools/curl.exe')]),
  ],
  topology: null,
}

const rpc = vi.hoisted(() => ({ reports: [] as ReportRequest[] }))

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
        total: usage(0, 0, 0),
        groups: [],
        other: usage(0, 0, 0),
        next: null,
      }
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
      createTestRoute(sourceRoute, '/source/', '/source/', topologyRoute),
      createTestRoute(inboundRoute, '/inbound/', '/inbound/', topologyRoute),
      createTestRoute(targetRoute, '/target/', '/target/', topologyRoute),
      createTestRoute(exitRoute, '/exit/', '/exit/', topologyRoute),
      createTestRoute(processRoute, '/process/', '/process/', topologyRoute),
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
    <QueryClientProvider client={queries}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )

  onFinished(async () => {
    await view.unmount()
    queries.clear()
    clearMocks()
    rpc.reports = []
    backend.intervals = []
    backend.retention = '7d'
    await page.viewport(1280, 720)
  })

  return view
}

const sidebarItem = (view: Awaited<ReturnType<typeof open>>, name: string) =>
  view
    .getByRole('complementary', { name: m.traffic_sidebar() })
    .getByRole('link', { name })

const summary = `5.00 KiB · ${m.topology_connection_count({ count: 12 })}`

test('traffic views fit at narrow, tablet and desktop widths', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished, {
    filters: [{ d: 'target', v: 'example.com' }],
  })
  const pageRoot = document.querySelector<HTMLElement>(
    '[data-slot="traffic-page"]',
  )!
  const sidebarElement = pageRoot.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar"]',
  )!
  const sidebarToggle = sidebarElement.querySelector<HTMLButtonElement>(
    '[data-slot="navigation-sidebar-toggle"]',
  )!
  const toolbar = pageRoot.querySelector<HTMLElement>(
    '[data-slot="traffic-toolbar"]',
  )!
  const dock = pageRoot.querySelector<HTMLElement>(
    '[data-slot="data-page-dock"]',
  )!
  const content = pageRoot.querySelector<HTMLElement>(
    '[data-slot="traffic-content"]',
  )!
  const scrollArea = content.closest<HTMLElement>('[data-slot="scroll-area"]')!
  const viewport = scrollArea.querySelector<HTMLElement>(
    '[data-slot="scroll-area-viewport"]',
  )!

  await document.fonts.ready
  expect(getComputedStyle(toolbar).fontFamily).toMatch(/^"Inter Variable"/)

  for (const width of [480, 800, 1280]) {
    await page.viewport(width, 720)
    await expect.poll(() => pageRoot.clientWidth).toBe(width)

    await sidebarItem(view, m.traffic_overview()).click()
    await expect
      .element(
        view.getByRole('heading', { name: m.traffic_overview(), level: 1 }),
      )
      .toBeInTheDocument()
    await expect
      .poll(
        () => document.querySelectorAll('[data-slot="traffic-stat"]').length,
      )
      .toBe(7)
    expect(
      document.querySelectorAll('[data-slot="traffic-ranking"]').length,
    ).toBe(5)

    await sidebarItem(view, m.topology_geo_map()).click()
    await expect
      .element(
        view.getByRole('heading', { name: m.topology_geo_map(), level: 1 }),
      )
      .toBeInTheDocument()
    await expect
      .poll(() => rpc.reports.at(-1)?.topology?.layers)
      .toEqual(['source_region', 'destination_region', 'destination_basis'])
    expect(document.querySelectorAll('[data-slot="traffic-stat"]').length).toBe(
      0,
    )
    expect(content.textContent).toContain(summary)

    const source = dimensionName('source')
    await sidebarItem(view, source).click()
    await expect
      .element(view.getByRole('heading', { name: source, level: 1 }))
      .toBeInTheDocument()
    await expect
      .element(view.getByRole('button', { name: /192\.168\.1\.2/ }))
      .toBeInTheDocument()
    expect(content.textContent).toContain(summary)
    await expect
      .element(sidebarItem(view, source))
      .toHaveAttribute('data-active', 'true')

    // A short category still fills the viewport, including its animated page.
    await expect
      .poll(() => getComputedStyle(content.lastElementChild!).transform)
      .toBe('none')
    expect(content.getBoundingClientRect().height).toBeGreaterThanOrEqual(
      viewport.clientHeight - 1,
    )
    const contentStyle = getComputedStyle(content)
    const outlet = content.lastElementChild as HTMLElement
    expect(outlet.getBoundingClientRect().bottom).toBeCloseTo(
      content.getBoundingClientRect().bottom -
        Number.parseFloat(contentStyle.paddingBottom),
      0,
    )

    sidebarToggle.click()
    await expect
      .poll(() => sidebarElement.getBoundingClientRect().width)
      .toBe(200)
    expect(sidebarToggle.getAttribute('aria-expanded')).toBe('true')
    sidebarToggle.click()
    await expect
      .poll(() => sidebarElement.getBoundingClientRect().width)
      .toBe(88)
    expect(sidebarToggle.getAttribute('aria-expanded')).toBe('false')

    const controls = toolbar.querySelector<HTMLElement>(
      '[data-slot="traffic-controls"]',
    )!
    expect(pageRoot.scrollWidth, `${width}px page`).toBeLessThanOrEqual(
      pageRoot.clientWidth,
    )
    expect(toolbar.scrollWidth, `${width}px toolbar`).toBeLessThanOrEqual(
      toolbar.clientWidth,
    )
    expect(
      controls.getBoundingClientRect().right,
      `${width}px controls`,
    ).toBeLessThanOrEqual(toolbar.getBoundingClientRect().right + 1)

    await sidebarItem(view, m.traffic_overview()).click()
    await expect
      .element(
        view.getByRole('heading', { name: m.traffic_overview(), level: 1 }),
      )
      .toBeInTheDocument()
    // The incoming page settles before measuring its final scroll extent.
    await expect
      .poll(() => getComputedStyle(content.lastElementChild!).transform)
      .toBe('none')
    viewport.scrollTop = 0
    if (width === 480) {
      await expect
        .poll(() => viewport.scrollHeight)
        .toBeGreaterThan(viewport.clientHeight)
    }

    if (viewport.scrollHeight > viewport.clientHeight) {
      viewport.scrollTop = viewport.scrollHeight
      await expect
        .poll(() =>
          Math.abs(
            viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight,
          ),
        )
        .toBeLessThan(2)
    }

    const lastEntry = pageRoot.querySelectorAll<HTMLElement>(
      '[data-slot="traffic-view-entry"]',
    )[1]
    const paddingBottom = Number.parseFloat(
      getComputedStyle(content).paddingBottom,
    )
    expect(paddingBottom).toBeGreaterThanOrEqual(dock.clientHeight + 32)
    expect(lastEntry.getBoundingClientRect().bottom).toBeLessThanOrEqual(
      dock.getBoundingClientRect().top - 8,
    )
  }
})

test('all eight sidebar entries select the matching page view', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  const entries = [
    ['overview', m.traffic_overview()],
    ['flow', m.topology_flow()],
    ['map', m.topology_geo_map()],
    ['source', dimensionName('source')],
    ['inbound', dimensionName('inbound')],
    ['target', dimensionName('target')],
    ['exit', dimensionName('exit')],
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
    } else if (value !== 'overview') {
      const ranking = report.rankings.find((item) => item.dimension === value)!
      const rowName = usageLabel(value, ranking.groups[0]!.key).text
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
