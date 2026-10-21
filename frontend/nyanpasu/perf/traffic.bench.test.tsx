import {
  Profiler,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  mockTrafficGroups,
  mockTrafficReport,
} from '@/pages/(main)/main/topology/_modules/mock-traffic'
import { Route as RankingsRoute } from '@/pages/(main)/main/topology/$dimension'
import { Route as FlowRoute } from '@/pages/(main)/main/topology/flow'
import { Route as OverviewRoute } from '@/pages/(main)/main/topology/index'
import { Route as MapRoute } from '@/pages/(main)/main/topology/map'
import { Route as TopologyRoute } from '@/pages/(main)/main/topology/route'
import type {
  Dimension,
  Metric,
  ReportRequest,
  TrafficQuery,
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
} from '@tanstack/react-router'
import { TestQueryProvider as QueryClientProvider } from '../tests/query-provider'
import { measureFrames, onProfilerRender, summarize } from './measure'

// Opens the traffic pages, feeds them polled reports, switches to them from
// the overview, loads more of the rankings listing and toggles a filter on
// the overview, reporting the frame drops of each. See `perf/README.md`.

const env = import.meta.env
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2
const THROTTLE = Number(env.VITE_PERF_THROTTLE ?? 1)
// Hosts the rankings page can list, loaded a page at a time as the real
// listing is; enough for every measured page load by default.
const GROUPS = Number(env.VITE_PERF_TRAFFIC_GROUPS ?? 1000)
const PAGE_SIZE = 50

declare module 'vitest/browser' {
  interface BrowserCommands {
    throttleCpu: (rate: number) => Promise<void>
    startCpuProfile: () => Promise<void>
    pauseCpuProfile: () => Promise<void>
    stopCpuProfile: (file: string) => Promise<void>
  }
}

// The perf config has no SVG component plugin.
vi.mock('@/assets/image/logo.svg?react', () => ({ default: () => null }))

/**
 * The reports the pages poll. A report is generated before the measured
 * action publishes it, so the generator's own time is not the page's.
 */
const reports = vi.hoisted(() => {
  let tick = 0
  const listeners = new Set<() => void>()
  const cache = new Map<string, unknown>()
  const requests = new Map<string, unknown>()

  return {
    tick: () => tick,
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    cache,
    requests,
    publish: (next: number) => {
      tick = next
      listeners.forEach((listener) => listener())
    },
  }
})

const NOW = 1_700_000_000_000
const reportAt = (request: ReportRequest, tick: number) =>
  mockTrafficReport(request, NOW + tick * 2000)

/** Generates every report the pages asked for, for the coming tick. */
function prepare(tick: number) {
  for (const [key, request] of reports.requests) {
    reports.cache.set(
      `${tick}:${key}`,
      reportAt(request as ReportRequest, tick),
    )
  }
}

// A long tail of hosts for the rankings page.
const hosts: UsageGroup[] = Array.from({ length: GROUPS }, (_, index) => ({
  key: `host-${index}.example.com`,
  usage: {
    bytes: {
      upload: (GROUPS - index) * 1_000,
      download: (GROUPS - index) * 9_000,
    },
    connections: GROUPS - index,
  },
  current_rate: null,
}))

const PROFILES = vi.hoisted(() => ({ query: { data: { items: [] } } }))

// The listing's next page, as its sentinel would ask for it.
const listing = vi.hoisted(() => ({ more: () => {} }))

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>()
  // Stable, as the store's own are.
  const kvState = { isLoading: false, readError: null, refresh: async () => {} }
  const setKv = async () => true

  return {
    ...original,
    // The view settings keep their defaults; there is no backend to store in.
    useKvStorage: (_key: string, defaultValue: unknown) =>
      [defaultValue, setKv, kvState] as const,
    // Traffic reports use RPC, not live connection-detail frames.
    ClashConnectionDetailsFreezeBoundary: ({
      children,
    }: {
      children: ReactNode
    }) => children,
    useSetting: () => ({ value: '30d' }),
    useLocalIpProbe: () => {},
    // Stable like a query's data, so the label of a key is too.
    useProfile: () => PROFILES,
    useTrafficReport: (request: ReportRequest) => {
      const tick = useSyncExternalStore(reports.subscribe, reports.tick)
      const key = JSON.stringify(request)

      return useMemo(() => {
        reports.requests.set(key, request)
        const cached = reports.cache.get(`${tick}:${key}`) as
          TrafficReport | undefined

        return { data: cached ?? reportAt(request, tick), isError: false }
        // eslint-disable-next-line react-hooks/exhaustive-deps
      }, [key, tick])
    },
    useTrafficUsagePages: (
      query: TrafficQuery,
      dimension: Dimension,
      metric: Metric,
    ) => {
      const [loaded, setLoaded] = useState(1)

      const all = useMemo(
        () =>
          dimension === 'target'
            ? hosts
            : mockTrafficGroups(query, dimension, metric, NOW),
        [query, dimension, metric],
      )
      // Each page keeps its identity, as React Query's pages do.
      const pages = useMemo(
        () =>
          Array.from(
            { length: Math.ceil(all.length / PAGE_SIZE) },
            (_, page) => ({
              total: { bytes: { upload: 0, download: 0 }, connections: 0 },
              groups: all.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE),
              other: { bytes: { upload: 0, download: 0 }, connections: 0 },
              next: null,
            }),
          ),
        [all],
      )
      listing.more = () => setLoaded((count) => count + 1)

      return useMemo(
        () => ({
          data: { pages: pages.slice(0, loaded) },
          isError: false,
          hasNextPage: loaded < pages.length,
          isFetchingNextPage: false,
          // The sentinel's own requests are left out: only measured actions load.
          fetchNextPage: () => {},
        }),
        [pages, loaded],
      )
    },
  }
})

function createTrafficRouter() {
  const client = new QueryClient()
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={client}>
        <TooltipProvider>
          <div
            className="bg-mixed-background flex flex-col"
            style={{ height: 850, width: 1350 }}
          >
            <Profiler id="app" onRender={onProfilerRender}>
              <div className="flex min-h-0 flex-1 flex-col overflow-hidden [&>div]:min-h-0 [&>div]:flex-1">
                <Outlet />
              </div>
            </Profiler>
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    ),
  })
  const mainRoute = createRoute({
    id: '/(main)',
    getParentRoute: () => rootRoute,
    component: Outlet,
  })
  const topologyRoute = TopologyRoute.update({
    id: '/main/topology',
    path: '/main/topology',
    getParentRoute: () => mainRoute,
  } as never)
  const child = (route: { update: (o: object) => unknown }, path: string) =>
    route.update({
      id: path,
      path,
      getParentRoute: () => topologyRoute,
    } as never)

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        (topologyRoute as unknown as typeof mainRoute).addChildren([
          child(OverviewRoute, '/') as never,
          child(FlowRoute, '/flow/') as never,
          child(MapRoute, '/map/') as never,
          child(RankingsRoute, '/$dimension/') as never,
        ]),
        createRoute({
          path: '/main/blank',
          getParentRoute: () => mainRoute,
          component: () => <div data-slot="blank-page" />,
        }) as never,
      ]),
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/blank'] }),
  })
}

const PAGES = (env.VITE_PERF_PAGES ?? 'overview,flow,map,rankings').split(',')
const PATHS: Record<string, string> = {
  overview: '/main/topology',
  flow: '/main/topology/flow',
  map: '/main/topology/map',
  rankings: '/main/topology/target',
}

const settle = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

// Every page at a 4x slowdown takes longer than the default five minutes.
test(`traffic pages ${PAGES.join(',')}`, async ({ onTestFinished }) => {
  const router = createTrafficRouter()
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  const go = (page: string) => router.navigate({ to: PATHS[page] as never })
  const home = () => router.navigate({ to: '/main/blank' as never })
  const report = (
    name: string,
    samples: Awaited<ReturnType<typeof measureFrames>>[],
  ) =>
    console.log(
      `PERF_REPORT ${JSON.stringify({
        browser: server.browser,
        action: name,
        throttle: THROTTLE,
        ...summarize(samples.slice(WARMUP)),
      })}`,
    )

  await expect
    .poll(() => container.querySelector('[data-slot="blank-page"]'))
    .toBeTruthy()
  // Open every page once so lazy modules are loaded before measuring.
  for (const page of PAGES) {
    await go(page)
    await settle(600)
    await home()
    await settle(400)
  }

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  let tick = 0

  for (const page of PAGES) {
    // Opening the page from another one.
    const opens = []
    for (let i = 1; i <= SWITCHES; i++) {
      prepare(tick)
      opens.push(
        await measureFrames(() => {
          go(page)
        }, 900),
      )
      await home()
      await settle(500)
    }
    report(`open:${page}`, opens)

    // A polled report arriving while the page is open.
    await go(page)
    await settle(900)
    const polls = []
    for (let i = 1; i <= SWITCHES; i++) {
      tick += 1
      prepare(tick)
      polls.push(await measureFrames(() => reports.publish(tick), 900))
    }
    report(`poll:${page}`, polls)

    // Switching to the page from the overview, as its sidebar entry does:
    // the traffic layout stays and the overview slides out.
    if (page !== 'overview') {
      const switches = []
      for (let i = 1; i <= SWITCHES; i++) {
        await go('overview')
        await settle(900)
        prepare(tick)
        switches.push(
          await measureFrames(() => {
            go(page)
          }, 900),
        )
      }
      report(`switch:${page}`, switches)
    }

    // The next page of the listing, below the rows already shown.
    if (page === 'rankings') {
      const appends = []
      for (let i = 1; i <= SWITCHES; i++) {
        appends.push(await measureFrames(() => listing.more(), 900))
      }
      report('append:rankings', appends)
    }

    await home()
    await settle(500)
  }

  // Six more openings, polls or switches of the first page, profiled.
  if (env.VITE_PERF_PROFILE) {
    const [page] = PAGES
    const action = env.VITE_PERF_PROFILE_ACTION ?? 'open'
    if (action !== 'open') {
      await go(action === 'poll' ? page : 'overview')
      await settle(900)
    }
    await commands.startCpuProfile()
    for (let i = 1; i <= 6; i++) {
      if (action === 'poll') {
        tick += 1
        prepare(tick)
        await measureFrames(() => reports.publish(tick), 900)
      } else if (action === 'switch') {
        prepare(tick)
        await measureFrames(() => {
          go(page)
        }, 900)
        // The way back to the overview is not part of the profile.
        await commands.pauseCpuProfile()
        await go('overview')
        await settle(900)
        await commands.startCpuProfile()
      } else {
        prepare(tick)
        await measureFrames(() => {
          go(page)
        }, 900)
        await home()
        await settle(500)
      }
    }
    await commands.stopCpuProfile(env.VITE_PERF_PROFILE)
    await home()
    await settle(500)
  }

  // A ranking row on the overview toggling its filter.
  if (PAGES.includes('overview')) {
    await go('overview')
    await expect
      .poll(() =>
        container.querySelector('[data-slot="traffic-ranking"] button'),
      )
      .toBeTruthy()
    const toggles = []
    for (let i = 1; i <= SWITCHES; i++) {
      // The first two toggles leave both reports generated for this tick.
      prepare(tick)
      const row = container.querySelector<HTMLButtonElement>(
        '[data-slot="traffic-ranking"] button[aria-pressed]',
      )!
      toggles.push(await measureFrames(() => row.click(), 900))
      await settle(300)
    }
    report('filter:overview', toggles)
  }
}, 900_000)
