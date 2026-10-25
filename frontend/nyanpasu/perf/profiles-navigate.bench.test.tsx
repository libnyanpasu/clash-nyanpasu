import { Profiler, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { Route as DetailRoute } from '@/pages/(main)/main/profiles/$type/detail/$uid'
import { Route as TypeIndexRoute } from '@/pages/(main)/main/profiles/$type/index'
import { Route as ProfilesRoute } from '@/pages/(main)/main/profiles/route'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from '../tests/query-provider'
import { createRuntimeYaml } from './fixtures/runtime-yaml'
import {
  measureFrames,
  onProfilerRender,
  summarize,
  type FrameSample,
} from './measure'

// Opens the profiles list from another section, moves between it and a
// profile's detail page, then refetches the profiles list with unchanged data,
// and reports the IPC calls and React render time each action causes, and the
// WebGL draws of the idle list. The pages run on the real React Query hooks
// against mocked IPC, so query caching is part of what is measured. See
// `perf/README.md`.

const env = import.meta.env
const PROFILES = Number(env.VITE_PERF_PROFILES ?? 30)
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2

declare module 'vitest/browser' {
  interface BrowserCommands {
    startCpuProfile: () => Promise<void>
    pauseCpuProfile: () => Promise<void>
    stopCpuProfile: (file: string) => Promise<void>
  }
}

vi.mock('@/components/providers/theme-provider', async () => {
  const { argbFromHex, themeFromSourceColor } =
    await import('@material/material-color-utilities')
  const context = {
    themePalette: themeFromSourceColor(argbFromHex('#1867C0')),
    themeCssVars: '',
    themeColor: '#1867C0',
    setThemeColor: async () => {},
    themeMode: 'light',
    currentThemeMode: 'light',
    setThemeMode: async () => {},
  }
  return { useExperimentalThemeContext: () => context }
})

// The RPC client calls Tauri, which `mockIPC` answers, only inside Tauri.
vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})

// The outlet freezes the clash streams of a leaving page; none run here.
vi.mock('@nyanpasu/query', async (importOriginal) => {
  const Through = ({ children }: { children: ReactNode }) => children

  return {
    ...(await importOriginal<object>()),
    ClashWSFreezeBoundary: Through,
    ClashConnectionDetailsFreezeBoundary: Through,
  }
})

const remote = (i: number) => ({
  type: 'config',
  uid: `p${i}`,
  name: `Subscription ${i}`,
  config: {
    type: 'file',
    source: {
      type: 'remote',
      file: `p${i}.yaml`,
      url: `https://example.com/${i}`,
      updated_at: 1_700_000_000,
      option: {
        with_proxy: false,
        self_proxy: false,
        update_interval_minutes: 0,
      },
      subscription: { upload: 1, download: 2, total: 100, expire: 0 },
    },
    transforms: [],
  },
})

const document_ = {
  current: 'p0',
  global_transforms: [],
  valid: [],
  items: Array.from({ length: PROFILES }, (_, i) => remote(i)),
}

// What the detail page summarizes: a subscription's nodes, groups and rules.
const profileFile = createRuntimeYaml(500, 3000)

const ipcCalls = new Map<string, number>()

function mockBackend() {
  mockIPC((wireCommand, args) => {
    const method =
      wireCommand === 'call_rpc'
        ? (args as { method: string }).method
        : wireCommand
    ipcCalls.set(method, (ipcCalls.get(method) ?? 0) + 1)
    switch (method) {
      case 'get_profiles':
        // A fresh copy, as IPC deserializes a new object every time.
        return structuredClone(document_)
      case 'read_profile_file':
        return profileFile
      case 'get_profile_sync_status':
        return {
          scheduled: false,
          next_run_at: null,
          active: [],
          journal_degraded: false,
          registration_error: null,
          history_limit: 10,
        }
      case 'get_profile_sync_runs':
      case 'get_profile_sync_logs':
        return { items: [], next: null }
      default:
        return null
    }
  })
}

function createProfilesRouter(queries: QueryClient) {
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <ContextMenuProvider>
              <ImportUrlProvider>
                <div
                  className="flex flex-col"
                  style={{ height: 850, width: 1350 }}
                >
                  <Profiler id="app" onRender={onProfilerRender}>
                    <div className="flex min-h-0 flex-1 flex-col overflow-hidden [&>div]:min-h-0 [&>div]:flex-1">
                      <Outlet />
                    </div>
                  </Profiler>
                </div>
              </ImportUrlProvider>
            </ContextMenuProvider>
          </TooltipProvider>
        </BlockTaskProvider>
      </QueryClientProvider>
    ),
  })

  // Same ids and paths as `route-tree.gen.ts`, so the pages' own
  // `Route.useParams()` resolve.
  const mainRoute = createRoute({
    id: '/(main)',
    getParentRoute: () => rootRoute,
    component: Outlet,
  })
  const profilesRoute = ProfilesRoute.update({
    id: '/main/profiles',
    path: '/main/profiles',
    getParentRoute: () => mainRoute,
  } as never)
  const typeIndexRoute = TypeIndexRoute.update({
    id: '/$type/',
    path: '/$type/',
    getParentRoute: () => profilesRoute,
  } as never)
  const detailRoute = DetailRoute.update({
    id: '/$type/detail/$uid',
    path: '/$type/detail/$uid',
    getParentRoute: () => profilesRoute,
  } as never)
  // Another section of the app, to open the profiles pages from.
  const otherRoute = createRoute({
    path: '/main/other',
    getParentRoute: () => mainRoute,
    component: () => null,
  })

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        (profilesRoute as unknown as typeof mainRoute).addChildren([
          typeIndexRoute as never,
          detailRoute as never,
        ]),
        otherRoute,
      ]),
    ]),
    history: createMemoryHistory({
      initialEntries: ['/main/profiles/profile'],
    }),
  })
}

const cardCount = (container: HTMLElement) =>
  container.querySelectorAll('[data-slot="profile-card"]').length

test(`navigate the profiles pages with ${PROFILES} profiles`, async ({
  onTestFinished,
}) => {
  mockBackend()
  const queries = new QueryClient()
  const router = createProfilesRouter(queries)
  const openList = () =>
    router.navigate({ to: '/main/profiles/profile' as never })
  const openDetail = () =>
    router.navigate({ to: '/main/profiles/profile/detail/p1' as never })
  const leave = () => router.navigate({ to: '/main/other' as never })

  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  await expect
    .poll(() => cardCount(container), { timeout: 20_000 })
    .toBe(PROFILES)
  // Load the detail page's lazy modules once before measuring.
  await openDetail()
  await expect
    .poll(() => container.querySelector('[data-slot="profile-card"]'))
    .toBeFalsy()
  await openList()
  await expect.poll(() => cardCount(container)).toBe(PROFILES)
  await new Promise((resolve) => setTimeout(resolve, 500))

  const measure = async (action: () => void, ms: number) => {
    ipcCalls.clear()
    const sample = await measureFrames(action, ms)
    return { sample, getProfiles: ipcCalls.get('get_profiles') ?? 0 }
  }

  const opens = []
  const details = []
  const lists = []
  const refetches = []

  for (let i = 1; i <= SWITCHES; i++) {
    await leave()
    await new Promise((resolve) => setTimeout(resolve, 300))
    opens.push(
      await measure(() => {
        openList()
      }, 900),
    )
    details.push(
      await measure(() => {
        openDetail()
      }, 900),
    )
    lists.push(
      await measure(() => {
        openList()
      }, 900),
    )
    // The state event or window focus path: same data, new objects.
    refetches.push(
      await measure(() => {
        queries.refetchQueries({ queryKey: ['getProfiles'] })
      }, 300),
    )
  }

  // Opening the list from another section, as `open` measures it. Leaving
  // happens between the recorded pieces.
  if (env.VITE_PERF_PROFILE) {
    for (let i = 1; i <= 6; i++) {
      await leave()
      await new Promise((resolve) => setTimeout(resolve, 300))
      await commands.startCpuProfile()
      await measureFrames(() => {
        openList()
      }, 900)
      if (i < 6) await commands.pauseCpuProfile()
    }
    await commands.stopCpuProfile(env.VITE_PERF_PROFILE)
    await openList()
    await expect.poll(() => cardCount(container)).toBe(PROFILES)
  }

  // WebGL draws while the list sits idle: an animated shader draws every
  // frame, a static one only when its uniforms change.
  let draws = 0
  for (const context of [WebGLRenderingContext, WebGL2RenderingContext]) {
    const drawArrays = context.prototype.drawArrays
    context.prototype.drawArrays = function (...args) {
      draws += 1
      return drawArrays.apply(this, args)
    }
  }
  await new Promise((resolve) => setTimeout(resolve, 2000))
  const idleDrawsPerSecond = draws / 2

  const summarizeActions = (
    list: { sample: FrameSample; getProfiles: number }[],
  ) => {
    const measured = list.slice(WARMUP)
    const getProfiles = measured.reduce(
      (total, item) => total + item.getProfiles,
      0,
    )

    return {
      getProfilesPerAction: Number((getProfiles / measured.length).toFixed(2)),
      ...summarize(measured.map((item) => item.sample)),
    }
  }

  const report = {
    browser: server.browser,
    profiles: PROFILES,
    idleDrawsPerSecond,
    // The list, sidebar included, opened from another section.
    open: summarizeActions(opens),
    detail: summarizeActions(details),
    // Back to the list from a detail page; the sidebar stays.
    list: summarizeActions(lists),
    unchangedRefetch: summarizeActions(refetches),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
