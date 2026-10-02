import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import type { ClashLog, LogRow } from '@nyanpasu/interface'
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
// Both sources show a bounded page; `app` shows a log file, whose
// first page arrives after the page has opened.
const SOURCE = (env.VITE_PERF_SOURCE ?? 'core') as 'core' | 'app'
const LOGS = Number(env.VITE_PERF_LOGS ?? 200)
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

// The logs the page shows. An arriving log replaces the array, as the
// websocket history and the file log hook do.
const store = vi.hoisted(() => ({
  core: null as unknown as {
    data: ClashLog[]
    isLoading: boolean
    error: null
    clean: { mutateAsync: () => Promise<void> }
    status: null
    more: boolean
    loadingOlder: boolean
    retry: () => void
    loadOlder: () => void
    detail: () => Promise<ClashLog['record']>
  },
  file: null as unknown as { rows: LogRow[] } & Record<string, unknown>,
  listeners: new Set<() => void>(),
  subscribe(listener: () => void) {
    store.listeners.add(listener)
    return () => store.listeners.delete(listener)
  },
  push(log: ClashLog | LogRow) {
    if ('record' in log) {
      const { data } = store.core
      store.core = { ...store.core, data: [...data.slice(1), log] }
    } else {
      const { rows } = store.file
      store.file = { ...store.file, rows: [...rows.slice(1), log] }
    }
    for (const listener of store.listeners) listener()
  },
}))

vi.mock('@nyanpasu/interface', async (importOriginal) => {
  const { useEffect, useState, useSyncExternalStore } = await import('react')
  const { createFileLogsFixture, createLogsFixture } =
    await import('./fixtures/logs')
  store.file = {
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
  const fileLogsLoading = { ...store.file, rows: [], loading: true }
  store.core = {
    data: createLogsFixture(Number(import.meta.env.VITE_PERF_LOGS ?? 200)),
    isLoading: false,
    error: null,
    clean: { mutateAsync: async () => {} },
    status: null,
    more: false,
    loadingOlder: false,
    retry: () => {},
    loadOlder: () => {},
    detail: async () => createLogsFixture(1)[0].record,
  }

  return {
    ...(await importOriginal<object>()),
    useClashLogs: () => useSyncExternalStore(store.subscribe, () => store.core),
    // Like the real hook, a mounted viewer starts empty and receives its
    // first page from IPC a moment later.
    useFileLogs: () => {
      const [loaded, setLoaded] = useState(false)
      const file = useSyncExternalStore(store.subscribe, () => store.file)
      useEffect(() => {
        const timer = setTimeout(() => setLoaded(true), 30)
        return () => clearTimeout(timer)
      }, [])
      return loaded ? file : fileLogsLoading
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

// With the logs page open and following, logs keep arriving; reports what
// each one costs.
test(`receive ${SOURCE} logs with ${LOGS} logs on the page`, async ({
  onTestFinished,
}) => {
  const router = createLogsRouter()
  await router.navigate({
    to: '/main/logs' as never,
    search: { source: SOURCE } as never,
  })

  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  await expect
    .poll(() => rowCount(container), { timeout: 20_000 })
    .toBeGreaterThan(3)
  await new Promise((resolve) => setTimeout(resolve, 500))

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  const { createFileLogsFixture, createLogsFixture } =
    await import('./fixtures/logs')
  const incoming = (
    SOURCE === 'core'
      ? createLogsFixture(LOGS + SWITCHES)
      : createFileLogsFixture(LOGS + SWITCHES)
  ).slice(LOGS)
  const samples = []

  for (const log of incoming) {
    samples.push(await measureFrames(() => store.push(log), 250))
  }

  const last = incoming.at(-1)!
  const text = 'record' in last ? last.record.payload : last.message
  // The page still follows the newest log.
  await expect
    .poll(() =>
      container
        .querySelector(
          `[data-slot="logs-virtual-item"][data-index="${LOGS - 1}"]`,
        )
        ?.textContent?.includes(text.slice(0, 40)),
    )
    .toBe(true)

  const report = {
    browser: server.browser,
    source: `${SOURCE}-steady`,
    logs: LOGS,
    throttle: THROTTLE,
    events: SWITCHES - WARMUP,
    ...summarize(samples.slice(WARMUP)),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
