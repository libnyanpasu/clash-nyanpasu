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
import { TRAFFIC_VIEW_KV_KEY } from '../src/pages/(main)/main/topology/_modules/view-settings'
import { Route as rankingsRoute } from '../src/pages/(main)/main/topology/$dimension'
import { Route as flowRoute } from '../src/pages/(main)/main/topology/flow'
import { Route as mapRoute } from '../src/pages/(main)/main/topology/map'
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
    // The KV store caches what it reads.
    localStorage.removeItem(`nyanpasu-kv-:${btoa(TRAFFIC_VIEW_KV_KEY)}`)
    rpc.storage.clear()
    rpc.reports = []
    backend.intervals = []
    backend.retention = '7d'
    backend.usage = []
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
      .toBe(8)
    expect(
      document.querySelectorAll('[data-slot="traffic-ranking"]').length,
    ).toBe(3)
    expect(
      document.querySelectorAll('[data-slot="traffic-single-values"]').length,
    ).toBe(1)

    await sidebarItem(view, m.topology_geo_map()).click()
    await expect
      .element(
        view.getByRole('heading', { name: m.topology_geo_map(), level: 1 }),
      )
      .toBeInTheDocument()
    await expect
      .poll(() => rpc.reports.at(-1)?.topology?.layers)
      .toEqual(['source_region', 'destination_region', 'destination_basis'])
    // The overview's stats leave with it once its slide has ended.
    await expect
      .poll(
        () => document.querySelectorAll('[data-slot="traffic-stat"]').length,
      )
      .toBe(0)
    expect(content.textContent).toContain(summary)

    const rankings = dimensionName('source')
    await sidebarItem(view, rankings).click()
    await expect
      .element(view.getByRole('heading', { name: rankings, level: 1 }))
      .toBeInTheDocument()
    await expect
      .poll(
        () =>
          document.querySelectorAll('[data-slot="traffic-ranking-list"]')
            .length,
      )
      .toBe(1)
    await expect
      .element(view.getByRole('button', { name: /192\.168\.1\.2/ }))
      .toBeInTheDocument()
    expect(content.textContent).toContain(summary)
    await expect
      .element(sidebarItem(view, rankings))
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

    const rankingCards = pageRoot.querySelectorAll<HTMLElement>(
      '[data-slot="traffic-ranking"], [data-slot="traffic-single-values"]',
    )
    const paddingBottom = Number.parseFloat(
      getComputedStyle(content).paddingBottom,
    )
    expect(paddingBottom).toBeGreaterThanOrEqual(dock.clientHeight + 32)
    // Masonry columns can place the lowest card before the last DOM entry.
    expect(rankingCards.length).toBe(4)
    for (const card of rankingCards) {
      expect(card.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        dock.getBoundingClientRect().top - 8,
      )
    }
  }
})

test('a ranking card keeps its height while its rows fill in', async ({
  onTestFinished,
}) => {
  mockBackend()
  const target = report.rankings.find(
    ({ dimension }) => dimension === 'target',
  )!
  const groups = target.groups
  target.groups = ['a', 'b', 'c', 'd'].map((key) => group(`${key}.example.com`))
  onTestFinished(() => {
    target.groups = groups
  })
  // A card's height in each commit that shows it before its rows.
  const before = new Map<string, number>()
  const observer = new MutationObserver(() => {
    for (const card of document.querySelectorAll<HTMLElement>(
      '[data-slot="traffic-ranking"]',
    )) {
      if (card.querySelector('[data-slot="traffic-ranking-rows-placeholder"]'))
        before.set(card.querySelector('h2')!.textContent!, card.offsetHeight)
    }
  })
  observer.observe(document.body, { childList: true, subtree: true })
  onTestFinished(() => observer.disconnect())

  const view = await open(onTestFinished)
  await expect
    .element(view.getByRole('button', { name: /d\.example\.com/ }))
    .toBeInTheDocument()

  expect(before.size).toBe(3)
  for (const card of document.querySelectorAll<HTMLElement>(
    '[data-slot="traffic-ranking"]',
  )) {
    expect(card.offsetHeight).toBe(
      before.get(card.querySelector('h2')!.textContent!),
    )
  }
})

test('the map keeps the height of its regions while they fill in', async ({
  onTestFinished,
}) => {
  mockBackend()
  // Narrow, where the list sits under the map and would move what follows.
  await page.viewport(480, 720)
  let before: number | undefined
  const observer = new MutationObserver(() => {
    const list = document.querySelector<HTMLElement>(
      '[data-slot="traffic-geography-regions"]',
    )
    if (
      list?.querySelector('[data-slot="traffic-geography-regions-placeholder"]')
    )
      before = list.offsetHeight
  })
  observer.observe(document.body, { childList: true, subtree: true })
  onTestFinished(() => observer.disconnect())

  await open(onTestFinished, { view: 'map' })
  await expect
    .poll(() =>
      document.querySelector('[data-slot="traffic-geography-regions"] button'),
    )
    .toBeTruthy()

  expect(before).toBeGreaterThan(0)
  expect(
    document.querySelector<HTMLElement>(
      '[data-slot="traffic-geography-regions"]',
    )!.offsetHeight,
  ).toBe(before)
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
  viewport.scrollTop = viewport.scrollHeight
  const last = view.getByRole('button', { name: /host-299\.example\.com/ })
  await expect.element(last).toBeInTheDocument()
  expect(rows.querySelectorAll('button').length).toBeLessThan(40)
  // The list starts below the page heading and tabs; the last row still
  // lands inside the viewport rather than beside a blank stretch.
  await expect
    .poll(() => {
      const bounds = viewport.getBoundingClientRect()
      const row = last.element().getBoundingClientRect()

      return row.top >= bounds.top && row.bottom <= bounds.bottom
    })
    .toBe(true)
})

// The menu sits in a portal over the page, where a pointer click waits on
// actionability checks that never settle in the test frame.
const pick = async (locator: { element: () => Element }) =>
  (locator.element() as HTMLElement).click()

test('the flow card keeps its height whichever rankings show under it', async ({
  onTestFinished,
}) => {
  mockBackend()
  // Tall enough to leave room under the rankings for the card to take.
  await page.viewport(1280, 1600)
  const view = await open(onTestFinished, { view: 'flow' })
  const cardHeight = () =>
    document.querySelector<HTMLElement>('[data-slot="flow-card"]')!.offsetHeight
  const rankingCards = () =>
    document.querySelectorAll(
      '[data-slot="traffic-view-rankings"] [data-slot="traffic-ranking"]',
    ).length

  await expect.poll(rankingCards).toBe(3)
  const height = cardHeight()
  expect(height).toBeGreaterThan(0)

  // All five take a second row of cards.
  await view.getByRole('button', { name: m.traffic_view_settings() }).click()
  for (const title of [m.traffic_rank_source(), m.traffic_rank_inbound()]) {
    await pick(view.getByRole('menuitemcheckbox', { name: title }))
  }
  await expect.poll(rankingCards).toBe(5)
  expect(cardHeight()).toBe(height)
  // The test ends once the settings are stored, not while they are on the way.
  await expect
    .poll(() => JSON.parse(rpc.storage.get(TRAFFIC_VIEW_KV_KEY) ?? 'null'))
    .toEqual(
      expect.objectContaining({
        rankings: ['process', 'target', 'exit', 'source', 'inbound'],
      }),
    )
})

test('with no rankings chosen, the flow card fills the page above a way back to them', async ({
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

  const content = document.querySelector<HTMLElement>(
    '[data-slot="traffic-content"]',
  )!
  const inset = parseFloat(getComputedStyle(content).paddingBottom)

  // The card grows down to the button under it, set apart by its margin, and
  // the button reaches the content's bottom padding.
  await expect
    .poll(() => {
      const card = document.querySelector<HTMLElement>(
        '[data-slot="flow-card"]',
      )!
      const rankings = document.querySelector<HTMLElement>(
        '[data-slot="traffic-view-rankings"]',
      )!

      return Math.max(
        content.getBoundingClientRect().bottom -
          inset -
          rankings.getBoundingClientRect().bottom,
        rankings.getBoundingClientRect().top -
          card.getBoundingClientRect().bottom -
          parseFloat(getComputedStyle(card.parentElement!).rowGap) -
          parseFloat(getComputedStyle(rankings).marginTop),
      )
    })
    .toBeLessThanOrEqual(1)

  await show.click()
  await pick(
    view.getByRole('menuitemcheckbox', { name: m.traffic_rank_process() }),
  )
  // The open menu hides the page from role queries.
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
