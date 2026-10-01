import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { commands, server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Route as ClashRoute } from '@/pages/(main)/main/settings/clash/route'
import { Route as SettingsRoute } from '@/pages/(main)/main/settings/route'
import { Route as SystemRoute } from '@/pages/(main)/main/settings/system/route'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { measureFrames, onProfilerRender, summarize } from './measure'

// Opens settings pages from an empty settings page and reports the frame
// drops each opening causes. See `perf/README.md`.

const env = import.meta.env
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 12)
const WARMUP = 2
const THROTTLE = Number(env.VITE_PERF_THROTTLE ?? 1)

declare module 'vitest/browser' {
  interface BrowserCommands {
    throttleCpu: (rate: number) => Promise<void>
  }
}

// The perf config has no SVG component plugin.
vi.mock('@/assets/image/logo.svg?react', () => ({ default: () => null }))

// The pages only need their settings to render: the setting hooks hand out
// plain values, and every other hook an idle query and mutation.

vi.mock('@nyanpasu/interface', async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>()
  const noop = async () => {}
  const upsert = Object.assign(async () => {}, {
    mutate: () => {},
    mutateAsync: noop,
    isPending: false,
  })
  const mutation = { isPending: false, mutate: () => {}, mutateAsync: noop }
  const known: Record<string | symbol, unknown> = {
    data: undefined,
    value: undefined,
    isPending: false,
    isLoading: false,
    isSuccess: true,
    error: null,
    query: { data: undefined, isPending: false, isLoading: false },
    upsert,
    mutate: () => {},
    mutateAsync: noop,
    refetch: noop,
    then: undefined,
  }
  // Any other field reads as an idle mutation.
  const generic: Record<string, unknown> = new Proxy(known, {
    get: (target, key) => (key in target ? target[key] : mutation),
  })
  const app: Record<string, unknown> = {
    theme_mode: 'light',
    theme_color: '#1867C0',
    language: 'en',
    core: 'mihomo',
    enable_system_proxy: false,
    enable_auto_launch: false,
    enable_silent_start: false,
    enable_proxy_guard: false,
    proxy_guard_interval: 30,
    system_proxy_bypass: 'localhost',
    app_log_level: 'info',
    log_level: 'info',
  }
  const clash: Record<string, unknown> = {
    enable_tun_mode: false,
    allow_lan: false,
    ipv6: false,
    mixed_port: { kind: 'fixed', start_port: 7890 },
    socks_port: null,
    http_port: null,
    log_level: 'info',
    tun_stack: 'mixed',
  }
  const settings = Object.fromEntries(
    Object.entries(app).map(([k, v]) => [k, { ...known, value: v }]),
  )
  const clashSettings = Object.fromEntries(
    Object.entries(clash).map(([k, v]) => [k, { ...known, value: v }]),
  )
  const hooks = Object.fromEntries(
    Object.keys(original)
      .filter(
        (key) =>
          /^use[A-Z]/.test(key) &&
          typeof original[key] === 'function' &&
          !['useClashWSHistory', 'useClashWSStatus'].includes(key),
      )
      .map((key) => [key, () => generic]),
  )
  return {
    ...original,
    ...hooks,
    useSetting: (key: string) => settings[key] ?? generic,
    useProxyMode: () => ({
      ...generic,
      value: { rule: true, global: false, direct: false },
    }),
    useClashSetting: (key: string) => clashSettings[key] ?? generic,
  }
})

function createSettingsRouter() {
  const client = new QueryClient()
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={client}>
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
      </QueryClientProvider>
    ),
  })
  const mainRoute = createRoute({
    id: '/(main)',
    getParentRoute: () => rootRoute,
    component: Outlet,
  })
  const settingsRoute = SettingsRoute.update({
    id: '/main/settings',
    path: '/main/settings',
    getParentRoute: () => mainRoute,
  } as never)
  const child = (route: { update: (o: object) => unknown }, path: string) =>
    route.update({
      id: path,
      path,
      getParentRoute: () => settingsRoute,
    } as never)

  return createRouter({
    routeTree: rootRoute.addChildren([
      mainRoute.addChildren([
        (settingsRoute as unknown as typeof mainRoute).addChildren([
          child(ClashRoute, '/clash') as never,
          child(SystemRoute, '/system') as never,
          createRoute({
            path: '/blank',
            getParentRoute: () => settingsRoute as never,
            component: () => <div data-slot="blank-page" />,
          }) as never,
        ]),
      ]),
    ]),
    history: createMemoryHistory({
      initialEntries: ['/main/settings/blank'],
    }),
  })
}

const PAGES = (env.VITE_PERF_PAGES ?? 'clash,system').split(',')

test(`switch settings pages ${PAGES.join(',')}`, async ({ onTestFinished }) => {
  const router = createSettingsRouter()
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  root.render(<RouterProvider router={router as never} />)
  onTestFinished(() => root.unmount())

  const go = (page: string) =>
    router.navigate({ to: `/main/settings/${page}` as never })
  const home = () => router.navigate({ to: '/main/settings/blank' as never })

  await expect
    .poll(() => container.querySelector('[data-slot="settings-content"]'))
    .toBeTruthy()
  // Open every page once so lazy modules are loaded before measuring.
  for (const page of PAGES) {
    await go(page)
    await new Promise((resolve) => setTimeout(resolve, 600))
    await home()
    await new Promise((resolve) => setTimeout(resolve, 600))
  }

  if (THROTTLE > 1) {
    await commands.throttleCpu(THROTTLE)
  }

  for (const page of PAGES) {
    const samples = []
    for (let i = 1; i <= SWITCHES; i++) {
      samples.push(
        await measureFrames(() => {
          go(page)
        }, 900),
      )
      await home()
      await new Promise((resolve) => setTimeout(resolve, 700))
    }
    console.log(
      `PERF_REPORT ${JSON.stringify({
        browser: server.browser,
        page,
        throttle: THROTTLE,
        ...summarize(samples.slice(WARMUP)),
      })}`,
    )
  }
})
