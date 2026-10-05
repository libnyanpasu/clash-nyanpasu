import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { Route as GroupRoute } from '@/pages/(main)/main/proxies/group/$name'
import { Route as ProxiesRoute } from '@/pages/(main)/main/proxies/route'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import {
  measureFrames,
  onProfilerRender,
  summarize,
  traceForcedReflows,
} from './measure'

// Switches between proxy groups of the main window's proxies page and
// reports the frame drops each switch causes. See `perf/README.md`.

const env = import.meta.env
const GROUPS = Number(env.VITE_PERF_GROUPS ?? 30)
const NODES = Number(env.VITE_PERF_NODES ?? 1500)
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2
const THROTTLE = Number(env.VITE_PERF_THROTTLE ?? 1)

declare module 'vitest/browser' {
  interface BrowserCommands {
    throttleCpu: (rate: number) => Promise<void>
    startCpuProfile: () => Promise<void>
    stopCpuProfile: (file: string) => Promise<void>
  }
}

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const { createProxiesFixture } = await import('./fixtures/proxies')
  const noop = async () => {}
  // Stable objects, as React Query hands out between refetches.
  const clashProxies = {
    proxies: {
      data: createProxiesFixture({
        groups: Number(import.meta.env.VITE_PERF_GROUPS ?? 30),
        nodes: Number(import.meta.env.VITE_PERF_NODES ?? 1500),
      }),
    },
    selectProxy: noop,
    updateProxiesDelay: { mutateAsync: noop },
    updateGroupDelay: { mutateAsync: noop },
  }
  const proxyMode = {
    value: { rule: true, global: false, direct: false },
    upsert: noop,
  }
  const connections = { data: [] }

  return {
    ...(await importOriginal<object>()),
    useClashProxies: () => clashProxies,
    useProxyMode: () => proxyMode,
    useClashConnections: () => connections,
    useCachedIcon: () => ({ data: undefined }),
    useTrayIcon: () => ({ data: undefined }),
  }
})

// The page's routes under a bare root that stands in for the app shell.
function createProxiesRouter() {
  const rootRoute = createRootRoute({
    component: () => (
      <TooltipProvider>
        <BlockTaskProvider>
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
        </BlockTaskProvider>
      </TooltipProvider>
    ),
  })

  // Same ids and paths as `route-tree.gen.ts`, so the pages' own
  // `Route.useParams()` resolve.
  const mainRoute = createRoute({
    id: '/(main)',
    getParentRoute: () => rootRoute,
    component: Outlet,
  })
  const proxiesRoute = ProxiesRoute.update({
    id: '/main/proxies',
    path: '/main/proxies',
    getParentRoute: () => mainRoute,
  } as never)
  const indexRoute = createRoute({
    path: '/',
    getParentRoute: () => proxiesRoute as never,
    component: () => null,
  })
  const groupRoute = GroupRoute.update({
    id: '/group/$name',
    path: '/group/$name',
    getParentRoute: () => proxiesRoute,
  } as never)

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        (proxiesRoute as unknown as typeof mainRoute).addChildren([
          indexRoute as never,
          groupRoute as never,
        ]),
      ]),
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/proxies'] }),
  })
}

test(`switch between ${GROUPS} groups of ${NODES} nodes`, async ({
  onTestFinished,
}) => {
  if (env.VITE_PERF_TRACE_REFLOW) {
    traceForcedReflows()
  }

  const router = createProxiesRouter()
  const openGroup = (index: number) =>
    router.navigate({
      to: '/main/proxies/group/$name' as never,
      params: { name: `Group ${index % GROUPS}` } as never,
    })

  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  // Open the first group from the index route, as the app does, so the
  // scroll viewport the node list virtualizes against is already mounted.
  await expect
    .poll(() => container.querySelector('[data-slot="proxies-container"]'))
    .toBeTruthy()
  await openGroup(0)
  await expect
    .poll(
      () =>
        container.querySelectorAll('[data-slot="proxies-virtual-item"]').length,
      { timeout: 20_000 },
    )
    .toBeGreaterThan(10)
  await new Promise((resolve) => setTimeout(resolve, 1000))

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const samples = []

  for (let i = 1; i <= SWITCHES; i++) {
    // Long enough for the 350 ms slide and the exiting page's unmount.
    samples.push(
      await measureFrames(() => {
        openGroup(i)
      }, 1200),
    )
  }

  if (env.VITE_PERF_PRINT_GAPS) {
    for (const sample of samples) {
      console.log(`GAPS ${sample.gaps.map((gap) => gap.toFixed(0)).join(',')}`)
    }
  }

  if (env.VITE_PERF_PROFILE) {
    await commands.startCpuProfile()

    for (let i = 1; i <= 6; i++) {
      await measureFrames(() => {
        openGroup(SWITCHES + i)
      }, 1000)
    }

    await commands.stopCpuProfile(env.VITE_PERF_PROFILE)
  }

  const report = {
    browser: server.browser,
    groups: GROUPS,
    nodes: NODES,
    throttle: THROTTLE,
    renderedNodes: container.querySelectorAll(
      '[data-slot="proxies-virtual-item"]',
    ).length,
    ...summarize(samples.slice(WARMUP)),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
