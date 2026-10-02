import { Profiler, type ProfilerOnRenderCallback } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import { rpc } from '@/services/rpc'
import { RpcProvider } from '@nyanpasu/query/provider'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { Route as DashboardIndexRoute } from '@/pages/(main)/main/dashboard/index'
import { Route as DashboardRoute } from '@/pages/(main)/main/dashboard/route'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { measureFrames, onProfilerRender, summarize } from './measure'

// Opens the main window's dashboard from another page, then feeds it
// websocket samples, and reports what each costs. See `perf/README.md`.

const env = import.meta.env
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2
const THROTTLE = Number(env.VITE_PERF_THROTTLE ?? 1)

declare module 'vitest/browser' {
  interface BrowserCommands {
    throttleCpu: (rate: number) => Promise<void>
  }
}

// The websocket histories the widgets read, replaced per sample as the
// websocket provider does.
const ws = vi.hoisted(() => ({
  traffic: [] as { up: number; down: number }[],
  memory: [] as { inuse: number; oslimit: number }[],
  connections: [] as Record<string, unknown>[],
  tick: 0,
  listeners: new Set<() => void>(),
  subscribe(listener: () => void) {
    ws.listeners.add(listener)
    return () => ws.listeners.delete(listener)
  },
  push() {
    const n = ++ws.tick
    ws.traffic = [...ws.traffic.slice(-119), { up: n * 1000, down: n * 3000 }]
    ws.memory = [...ws.memory.slice(-119), { inuse: n * 1e6, oslimit: 0 }]
    ws.connections = [
      ...ws.connections.slice(-31),
      {
        downloadTotal: n * 1e7,
        uploadTotal: n * 1e6,
        downloadSpeed: n * 3000,
        uploadSpeed: n * 1000,
        memory: null,
        connectionCount: n % 50,
      },
    ]
    for (const listener of ws.listeners) listener()
  },
}))

// `useKvStorage` runs for real, against a local cache and a backend that
// answers after a moment, as IPC does. Both hold the user's saved layout.
const savedLayouts = vi.hoisted(() => {
  const widget = (type: string, x: number, y: number, w: number, h = 2) => ({
    id: type,
    type,
    x,
    y,
    w,
    h,
  })
  return JSON.stringify({
    '12x6': [
      widget('traffic-down', 0, 0, 3),
      widget('traffic-up', 3, 0, 3),
      widget('memory', 6, 0, 3),
      widget('connections', 9, 0, 3),
      widget('proxy-shortcuts', 0, 2, 3, 3),
      widget('core-shortcuts', 3, 2, 4),
    ],
  })
})
localStorage.setItem(`nyanpasu-kv-:${btoa('dashboard-widgets')}`, savedLayouts)
vi.mock('@/services/rpc', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/services/rpc')>()
  const ok = <T,>(data: T) => ({ status: 'ok' as const, data })
  return {
    ...original,
    rpc: {
      ...original.rpc,
      getStorageItem: async () => {
        await new Promise((resolve) => setTimeout(resolve, 20))
        return ok(savedLayouts)
      },
      setStorageItem: async () => ok(null),
      listenResync: () => () => {},
      events: {
        ...original.rpc.events,
        storageValueChangedEvent: { listen: async () => () => {} },
      },
    },
  }
})

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const { useSyncExternalStore } = await import('react')
  const noop = async () => {}
  const setting = (value: unknown) => ({ value, upsert: noop })
  const settings: Record<string, ReturnType<typeof setting>> = {
    core: setting('mihomo'),
    enable_system_proxy: setting(true),
    enable_tun_mode: setting(false),
  }
  const query = <T,>(data: T) => ({ data, isPending: false, isSuccess: true })
  // Stable objects, as React Query hands out between refetches.
  const systemProxy = query({ enable: true, server: '127.0.0.1:7890' })
  const clashConfig = { query: query({ 'mixed-port': 7890 }), upsert: noop }
  const coreStatus = query({
    status: 'Running',
    type: 'normal',
    controller: { Http: '127.0.0.1:9090' },
  })
  const systemService = {
    query: query({ status: 'not_installed', server: null }),
    upsert: { mutateAsync: noop },
  }
  const coreVersion = query('v1.19.0')
  const useHistory = <K extends 'traffic' | 'memory' | 'connections'>(
    kind: K,
  ) => ({
    data: useSyncExternalStore(ws.subscribe, () => ws[kind]),
    isLoading: false,
    error: null,
  })

  return {
    ...(await importOriginal<object>()),
    useSetting: (key: string) => settings[key],
    useClashSetting: (key: string) => settings[key],
    useSystemProxy: () => systemProxy,
    useClashConfig: () => clashConfig,
    useCoreStatus: () => coreStatus,
    useSystemService: () => systemService,
    useClashCoreVersion: () => coreVersion,
    useClashTraffic: () => useHistory('traffic'),
    useClashMemory: () => useHistory('memory'),
    useClashConnections: () => useHistory('connections'),
  }
})

let commits = 0
const onRender: ProfilerOnRenderCallback = (...args) => {
  commits += 1
  onProfilerRender(...args)
}

// The page's routes and an empty page to come from, under a bare root that
// stands in for the app shell.
function createDashboardRouter() {
  const rootRoute = createRootRoute({
    component: () => (
      <TooltipProvider>
        <BlockTaskProvider>
          <ContextMenuProvider>
            <div
              className="bg-mixed-background flex flex-col"
              style={{ height: 850, width: 1350 }}
            >
              <Profiler id="app" onRender={onRender}>
                <div className="flex min-h-0 flex-1 flex-col overflow-hidden [&>div]:min-h-0 [&>div]:flex-1">
                  <Outlet />
                </div>
              </Profiler>
            </div>
          </ContextMenuProvider>
        </BlockTaskProvider>
      </TooltipProvider>
    ),
  })

  // Same ids and paths as `route-tree.gen.ts`.
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
  const dashboardRoute = DashboardRoute.update({
    id: '/main/dashboard',
    path: '/main/dashboard',
    getParentRoute: () => mainRoute,
  } as never)
  const dashboardIndexRoute = DashboardIndexRoute.update({
    id: '/',
    path: '/',
    getParentRoute: () => dashboardRoute,
  } as never)

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        otherRoute,
        (dashboardRoute as unknown as typeof mainRoute).addChildren([
          dashboardIndexRoute as never,
        ]),
      ]),
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/other'] }),
  })
}

const widgetCount = (container: HTMLElement) =>
  container.querySelectorAll('[data-slot="widget-sparkline-card"]').length

async function mount(onTestFinished: (fn: () => void) => void) {
  const router = createDashboardRouter()
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(
    <RpcProvider rpc={rpc}>
      <RouterProvider router={router as never} />
    </RpcProvider>,
  )
  onTestFinished(() => root.unmount())
  await expect
    .poll(() => container.querySelector('[data-slot="other-page"]'))
    .toBeTruthy()
  for (let i = 0; i < 5; i++) ws.push()
  return { router, container }
}

test('open the dashboard', async ({ onTestFinished }) => {
  const { router, container } = await mount(onTestFinished)
  const openDashboard = () =>
    router.navigate({ to: '/main/dashboard' as never })
  const leave = () => router.navigate({ to: '/main/other' as never })

  // Open once so lazy modules are loaded before measuring.
  await openDashboard()
  await expect.poll(() => widgetCount(container)).toBe(4)
  await leave()

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const samples = []
  const commitCounts = []
  for (let i = 1; i <= SWITCHES; i++) {
    commits = 0
    samples.push(
      await measureFrames(() => {
        openDashboard()
      }, 800),
    )
    commitCounts.push(commits)
    await leave()
    await new Promise((resolve) => setTimeout(resolve, 200))
  }

  const measured = commitCounts.slice(WARMUP)
  console.log(
    `PERF_REPORT ${JSON.stringify({
      browser: server.browser,
      action: 'open',
      throttle: THROTTLE,
      avgCommits: measured.reduce((a, b) => a + b, 0) / measured.length,
      ...summarize(samples.slice(WARMUP)),
    })}`,
  )
})

// The websocket delivers traffic, memory and connection samples about once a
// second each; reports what one round of them costs an open dashboard.
test('receive websocket samples on the dashboard', async ({
  onTestFinished,
}) => {
  const { router, container } = await mount(onTestFinished)
  await router.navigate({ to: '/main/dashboard' as never })
  await expect.poll(() => widgetCount(container)).toBe(4)
  await new Promise((resolve) => setTimeout(resolve, 500))

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const samples = []
  for (let i = 1; i <= SWITCHES; i++) {
    samples.push(await measureFrames(() => ws.push(), 250))
  }

  console.log(
    `PERF_REPORT ${JSON.stringify({
      browser: server.browser,
      action: 'sample',
      throttle: THROTTLE,
      ...summarize(samples.slice(WARMUP)),
    })}`,
  )
})
