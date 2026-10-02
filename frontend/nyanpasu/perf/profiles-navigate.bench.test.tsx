import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
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
import { measureFrames, onProfilerRender, summarize } from './measure'

// Moves between the profiles list and a profile's detail page, then refetches
// the profiles list with unchanged data, and reports the IPC calls and React
// render time each action causes, and the WebGL draws of the idle list. The
// pages run on the real React Query hooks against mocked IPC, so query caching
// is part of what is measured. See `perf/README.md`.

const env = import.meta.env
const PROFILES = Number(env.VITE_PERF_PROFILES ?? 30)
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2

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

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        (profilesRoute as unknown as typeof mainRoute).addChildren([
          typeIndexRoute as never,
          detailRoute as never,
        ]),
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

  const navigations = []
  const refetches = []

  for (let i = 1; i <= SWITCHES; i++) {
    navigations.push(
      await measure(() => {
        openDetail()
      }, 900),
    )
    navigations.push(
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

  const sum = (list: { getProfiles: number }[]) =>
    list.reduce((total, item) => total + item.getProfiles, 0)
  const measured = navigations.slice(WARMUP * 2)
  const measuredRefetches = refetches.slice(WARMUP)

  const report = {
    browser: server.browser,
    profiles: PROFILES,
    idleDrawsPerSecond,
    navigation: {
      getProfilesPerNavigation: Number(
        (sum(measured) / measured.length).toFixed(2),
      ),
      ...summarize(measured.map((item) => item.sample)),
    },
    unchangedRefetch: summarize(measuredRefetches.map((item) => item.sample)),
  }

  console.log(`PERF_REPORT ${JSON.stringify(report)}`)
})
