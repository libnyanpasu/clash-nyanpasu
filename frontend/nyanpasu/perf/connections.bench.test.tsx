import { Profiler, useState, type Context, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Route as ConnectionsIndexRoute } from '@/pages/(main)/main/connections/index'
import { Route as ConnectionsRoute } from '@/pages/(main)/main/connections/route'
import { Route as RulesIndexRoute } from '@/pages/(main)/main/rules/index'
import { Route as RulesRoute } from '@/pages/(main)/main/rules/route'
import type { ClashConnectionDetails_Serialize } from '@nyanpasu/interface'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import {
  createConnectionStream,
  createRulesFixture,
  createUsageFixture,
} from './fixtures/connections'
import {
  measureFrames,
  onProfilerRender,
  summarize,
  traceForcedReflows,
  type FrameSample,
} from './measure'

// Opens the main window's connections or rules page from another page, then
// streams connection-detail frames into it, and reports the frame drops of
// each opening and each frame. See `perf/README.md`.

const env = import.meta.env
const PAGE = (env.VITE_PERF_PAGE ?? 'connections') as 'connections' | 'rules'
const CONNECTIONS = Number(env.VITE_PERF_CONNECTIONS ?? 2000)
const RULES = Number(env.VITE_PERF_RULES ?? 3000)
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const FRAMES = Number(env.VITE_PERF_FRAMES ?? 12)
const SEARCH = env.VITE_PERF_SEARCH ?? ''
const SORT = env.VITE_PERF_SORT ?? ''
const DETAIL = env.VITE_PERF_DETAIL === '1'
const WARMUP = 2
const THROTTLE = Number(env.VITE_PERF_THROTTLE ?? 1)

declare module 'vitest/browser' {
  interface BrowserCommands {
    throttleCpu: (rate: number) => Promise<void>
    startCpuProfile: () => Promise<void>
    stopCpuProfile: (file: string) => Promise<void>
  }
}

// The frame the pages read, provided by `FrameProvider` below.
const stream = vi.hoisted(() => ({
  Frame: null as unknown as Context<ClashConnectionDetails_Serialize | null>,
  publish: (_frame: ClashConnectionDetails_Serialize) => {},
}))

vi.mock('@nyanpasu/interface', async (importOriginal) => {
  const { createContext, useContext } = await import('react')
  const { createRulesFixture } = await import('./fixtures/connections')

  stream.Frame = createContext<ClashConnectionDetails_Serialize | null>(null)

  // Stable objects, as React Query hands out between refetches.
  const clashRules = {
    data: {
      rules: createRulesFixture(
        Number(import.meta.env.VITE_PERF_RULES ?? 3000),
      ),
    },
  }
  const clashProxies = {
    proxies: {
      data: {
        groups: Array.from({ length: 20 }, (_, i) => ({
          name: `Group ${i}`,
          icon: null,
        })),
      },
    },
  }
  // The status tabs' count; the page itself reads the detail frames.
  const connections = { data: [] }

  return {
    ...(await importOriginal<object>()),
    useClashRules: () => clashRules,
    useClashProxies: () => clashProxies,
    useClashConnections: () => connections,
    useClashConnectionDetails: () => {
      const data = useContext(stream.Frame)
      return { data, isLoading: data === null }
    },
  }
})

// Holds the latest frame in state like the real provider, so a frame is an
// update outside any event, as the IPC channel's callback makes it.
function FrameProvider({
  initial,
  children,
}: {
  initial: ClashConnectionDetails_Serialize
  children: ReactNode
}) {
  const [frame, setFrame] = useState(initial)
  stream.publish = setFrame

  return <stream.Frame.Provider value={frame}>{children}</stream.Frame.Provider>
}

// The pages' routes and an empty page to come from, under a bare root that
// stands in for the app shell.
function createPagesRouter(initial: ClashConnectionDetails_Serialize) {
  const queryClient = new QueryClient()

  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <FrameProvider initial={initial}>
          <TooltipProvider>
            <ContextMenuProvider>
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
            </ContextMenuProvider>
          </TooltipProvider>
        </FrameProvider>
      </QueryClientProvider>
    ),
  })

  // Same ids and paths as `route-tree.gen.ts`, so the pages' own
  // `Route.useSearch()` resolve.
  const mainRoute = createRoute({
    id: '/(main)',
    getParentRoute: () => rootRoute,
    component: Outlet,
  })
  const otherRoute = createRoute({
    path: '/main/other',
    getParentRoute: () => mainRoute,
    component: () => <div data-slot="other-page" />,
  })
  const pageRoutes = (
    [
      ['/main/connections', ConnectionsRoute, ConnectionsIndexRoute],
      ['/main/rules', RulesRoute, RulesIndexRoute],
    ] as const
  ).map(([path, layout, index]) => {
    const layoutRoute = (layout as typeof ConnectionsRoute).update({
      id: path,
      path,
      getParentRoute: () => mainRoute,
    } as never)
    const indexRoute = (index as typeof ConnectionsIndexRoute).update({
      id: '/',
      path: '/',
      getParentRoute: () => layoutRoute,
    } as never)

    return (layoutRoute as unknown as typeof mainRoute).addChildren([
      indexRoute as never,
    ])
  })

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([otherRoute, ...pageRoutes]),
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/other'] }),
  })
}

const ROW_SELECTOR =
  PAGE === 'rules'
    ? '[data-slot="rules-row"]'
    : '[data-slot="connections-virtual-tbody"] tr'

const rowCount = (container: HTMLElement) =>
  container.querySelectorAll(ROW_SELECTOR).length

// Types into a controlled input the way a keystroke does.
function typeInto(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    'value',
  )!.set!.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

function clickHeader(container: HTMLElement, label: string) {
  const header = [
    ...container.querySelectorAll<HTMLElement>(
      PAGE === 'rules'
        ? '[data-slot="rules-header"] button'
        : '[data-slot="connections-virtual-table"] th > div',
    ),
  ].find((element) => element.textContent?.trim() === label)

  if (!header) {
    throw new Error(`no column header named ${label}`)
  }

  header.click()
}

// Medians next to the averages: other work on the machine stretches a few
// samples far more than it moves the middle one.
function withMedians(samples: FrameSample[]) {
  const median = (key: 'maxFrameGap' | 'syncTime' | 'reactRender') => {
    const sorted = samples.map((sample) => sample[key]).sort((a, b) => a - b)
    return Number(sorted[Math.floor(sorted.length / 2)].toFixed(1))
  }

  return {
    ...summarize(samples),
    medMaxFrameGap: median('maxFrameGap'),
    medSyncTime: median('syncTime'),
    medReactRender: median('reactRender'),
  }
}

test(`stream ${CONNECTIONS} connections into the ${PAGE} page with ${RULES} rules`, async ({
  onTestFinished,
}) => {
  if (env.VITE_PERF_TRACE_REFLOW) {
    traceForcedReflows()
  }

  const rules = createRulesFixture(RULES)
  const nextFrame = createConnectionStream(CONNECTIONS, rules)
  let frame = 0

  // Session totals for the rules page, answered like the traffic store.
  mockIPC((cmd, args) => {
    const { method, params } = (args ?? {}) as {
      method?: string
      params?: { keys?: string[] }
    }

    if (cmd === 'call_rpc' && method === 'query_traffic_usage_by_keys') {
      return createUsageFixture(params?.keys ?? [])
    }

    return null
  })

  const router = createPagesRouter(nextFrame(frame))
  const openPage = () => router.navigate({ to: `/main/${PAGE}` as never })
  const leavePage = () => router.navigate({ to: '/main/other' as never })

  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  await expect
    .poll(() => container.querySelector('[data-slot="other-page"]'))
    .toBeTruthy()

  // Open once so lazy modules and fonts are loaded before measuring.
  await openPage()
  await expect
    .poll(() => rowCount(container), { timeout: 20_000 })
    .toBeGreaterThan(3)
  await leavePage()
  await new Promise((resolve) => setTimeout(resolve, 500))

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const opens: FrameSample[] = []
  // Rows shown at the end of each opening, before any new frame arrives.
  const openRows: number[] = []

  for (let i = 1; i <= SWITCHES; i++) {
    opens.push(
      await measureFrames(() => {
        openPage()
      }, 1200),
    )
    openRows.push(rowCount(container))
    await leavePage()
    await new Promise((resolve) => setTimeout(resolve, 300))
  }

  const publishNext = () => stream.publish(nextFrame(++frame))

  await openPage()
  publishNext()
  await expect.poll(() => rowCount(container)).toBeGreaterThan(3)

  // Each keystroke of the search term is measured on its own.
  const keystrokes: FrameSample[] = []

  for (let i = 1; i <= SEARCH.length; i++) {
    keystrokes.push(
      await measureFrames(() => {
        typeInto(
          container.querySelector('input[type="text"]')!,
          SEARCH.slice(0, i),
        )
      }, 500),
    )
  }

  if (SORT) {
    clickHeader(container, SORT)
  }

  if (DETAIL) {
    container
      .querySelector(ROW_SELECTOR)!
      .dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    await expect
      .poll(() => document.querySelector('[role="dialog"]'))
      .toBeTruthy()
  }

  await new Promise((resolve) => setTimeout(resolve, 1000))

  const frames: FrameSample[] = []

  for (let i = 1; i <= FRAMES; i++) {
    frames.push(await measureFrames(publishNext, 1000))
  }

  if (env.VITE_PERF_PRINT_GAPS) {
    for (const sample of [...opens, ...frames]) {
      console.log(`GAPS ${sample.gaps.map((gap) => gap.toFixed(0)).join(',')}`)
    }
  }

  if (env.VITE_PERF_PROFILE) {
    await commands.startCpuProfile()

    for (let i = 1; i <= 6; i++) {
      await measureFrames(publishNext, 1000)
    }

    await commands.stopCpuProfile(env.VITE_PERF_PROFILE)
  }

  const report = {
    browser: server.browser,
    page: PAGE,
    connections: CONNECTIONS,
    rules: RULES,
    search: SEARCH,
    sort: SORT,
    detail: DETAIL,
    throttle: THROTTLE,
    renderedRows: rowCount(container),
    openRows: Math.min(...openRows.slice(WARMUP)),
    open: withMedians(opens.slice(WARMUP)),
    ...(SEARCH && { keystroke: withMedians(keystrokes) }),
    frame: withMedians(frames.slice(WARMUP)),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
