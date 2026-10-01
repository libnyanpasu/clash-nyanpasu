import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Route as LogsIndexRoute } from '@/pages/(main)/main/logs/index'
import { Route as LogsRoute } from '@/pages/(main)/main/logs/route'
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

// Opens the main window's logs page from another page and reports the frame
// drops each opening causes. See `perf/README.md`.

const env = import.meta.env
// `core` shows the core's websocket history; `app` shows a log file, whose
// first page arrives after the page has opened.
const SOURCE = (env.VITE_PERF_SOURCE ?? 'core') as 'core' | 'app'
const LOGS = Number(env.VITE_PERF_LOGS ?? (SOURCE === 'core' ? 1024 : 200))
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

vi.mock('@nyanpasu/interface', async (importOriginal) => {
  const { useEffect, useState } = await import('react')
  const { createFileLogsFixture, createLogsFixture } =
    await import('./fixtures/logs')
  const fileLogs = {
    rows: createFileLogsFixture(Number(import.meta.env.VITE_PERF_LOGS ?? 200)),
    files: [],
    page: null,
    error: null,
    loadingOlder: false,
    loading: false,
    more: false,
    loadOlder: () => {},
    clear: () => {},
    latest: () => {},
    retry: () => {},
  }
  const fileLogsLoading = { ...fileLogs, rows: [], loading: true }
  // A stable object, as the websocket history context hands out between
  // unrelated events.
  const clashLogs = {
    data: createLogsFixture(Number(import.meta.env.VITE_PERF_LOGS ?? 1024)),
    isLoading: false,
    error: null,
    clean: { mutateAsync: async () => {} },
  }

  return {
    ...(await importOriginal<object>()),
    useClashLogs: () => clashLogs,
    // Like the real hook, a mounted viewer starts empty and receives its
    // first page from IPC a moment later.
    useFileLogs: () => {
      const [view, setView] = useState<typeof fileLogs>(fileLogsLoading)
      useEffect(() => {
        const timer = setTimeout(() => setView(fileLogs), 30)
        return () => clearTimeout(timer)
      }, [])
      return view
    },
  }
})

// The page's routes and an empty page to come from, under a bare root that
// stands in for the app shell.
function createLogsRouter() {
  const rootRoute = createRootRoute({
    component: () => (
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
  const logsRoute = LogsRoute.update({
    id: '/main/logs',
    path: '/main/logs',
    getParentRoute: () => mainRoute,
  } as never)
  const logsIndexRoute = LogsIndexRoute.update({
    id: '/',
    path: '/',
    getParentRoute: () => logsRoute,
  } as never)

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        otherRoute,
        (logsRoute as unknown as typeof mainRoute).addChildren([
          logsIndexRoute as never,
        ]),
      ]),
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/other'] }),
  })
}

const rowCount = (container: HTMLElement) =>
  container.querySelectorAll('[data-slot="logs-virtual-item"]').length

test(`open the ${SOURCE} logs page with ${LOGS} logs`, async ({
  onTestFinished,
}) => {
  if (env.VITE_PERF_TRACE_REFLOW) {
    traceForcedReflows()
  }

  const router = createLogsRouter()
  const openLogs = () =>
    router.navigate({
      to: '/main/logs' as never,
      search: { source: SOURCE } as never,
    })
  const leaveLogs = () => router.navigate({ to: '/main/other' as never })

  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  await expect
    .poll(() => container.querySelector('[data-slot="other-page"]'))
    .toBeTruthy()

  // Open once so lazy modules and fonts are loaded before measuring.
  await openLogs()
  await expect
    .poll(() => rowCount(container), { timeout: 20_000 })
    .toBeGreaterThan(3)
  await leaveLogs()
  await new Promise((resolve) => setTimeout(resolve, 500))

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const samples = []

  for (let i = 1; i <= SWITCHES; i++) {
    samples.push(
      await measureFrames(() => {
        openLogs()
      }, 1200),
    )
    await leaveLogs()
    await new Promise((resolve) => setTimeout(resolve, 300))
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
        openLogs()
      }, 1000)
      await leaveLogs()
      await new Promise((resolve) => setTimeout(resolve, 300))
    }

    await commands.stopCpuProfile(env.VITE_PERF_PROFILE)
  }

  await openLogs()
  // The page opens following the latest log.
  await expect
    .poll(() =>
      container.querySelector(
        `[data-slot="logs-virtual-item"][data-index="${LOGS - 1}"]`,
      ),
    )
    .toBeTruthy()

  const report = {
    browser: server.browser,
    source: SOURCE,
    logs: LOGS,
    throttle: THROTTLE,
    renderedRows: rowCount(container),
    ...summarize(samples.slice(WARMUP)),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
