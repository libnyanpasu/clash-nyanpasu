import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { act, useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import type { DashboardItem } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { NetworkInfoWidget } from '@/components/widgets/widget-network-info'
import { m } from '@/paraglide/messages'
import type { routeTree } from '@/route-tree.gen'
import { regionName } from '@/utils/traffic-usage'
import { DndContext } from '@dnd-kit/core'
import {
  createIconTransport,
  createRpcClient,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import type { EgressMethod, RouteEgress } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { TestQueryProvider } from './query-provider'

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createRouter<typeof routeTree>>
  }
}

const items: DashboardItem[] = [
  { id: 'network', type: WidgetId.NetworkInfo, x: 0, y: 0, w: 4, h: 2 },
]

const probed = (method: EgressMethod): RouteEgress => ({
  kind: 'probed',
  method,
  ipv4: {
    address: '203.0.113.7',
    country: 'JP',
    source: {
      kind: 'stun',
      host: 'stun.cloudflare.com',
      port: 3478,
      transport: 'udp',
    },
  },
  ipv6: null,
})

const PROBED = probed('direct')

const METHOD_LABELS: Record<EgressMethod, () => string> = {
  system_proxy: m.dashboard_widget_network_info_method_system_proxy,
  tun: m.dashboard_widget_network_info_method_tun,
  transparent_proxy: m.dashboard_widget_network_info_method_transparent_proxy,
  direct: m.dashboard_widget_network_info_method_direct,
}

/** The line naming the route and what answered over it. */
const routeText = (method: EgressMethod) =>
  `${METHOD_LABELS[method]()} · ${m.dashboard_widget_network_info_via_stun({
    server: 'stun.cloudflare.com:3478',
    transport: 'UDP',
  })}`

type Backend = {
  allowed: boolean
  tun: boolean
  systemProxy?: boolean
  hideAddresses?: boolean
  localCapture?: boolean
  probeImpl?: () => Promise<RouteEgress>
}

function createRpcHarness({
  allowed,
  tun,
  systemProxy,
  hideAddresses,
  localCapture,
  probeImpl,
}: Backend) {
  const calls: string[] = []
  const invoke = async <T,>(
    method: string,
    params?: Record<string, unknown>,
  ) => {
    calls.push(method)
    switch (method) {
      case 'get_storage_item':
        return {
          value:
            hideAddresses && String(params?.key).includes('config')
              ? JSON.stringify({
                  version: 1,
                  byInstance: {
                    network: { type: WidgetId.NetworkInfo, hideAddresses },
                  },
                })
              : null,
          revision: 0,
        } as T
      case 'set_storage_item':
        return 1 as T
      case 'get_app_settings':
        return {
          enable_local_ip_probe: allowed,
          enable_system_proxy: systemProxy ?? false,
        } as T
      case 'get_clash_settings':
        return {
          enable_tun_mode: tun,
          transparent_proxy: {
            mode: localCapture ? 'tproxy' : 'disabled',
            local: true,
          },
        } as T
      case 'probe_route_egress':
        return (probeImpl ? await probeImpl() : PROBED) as T
      default:
        throw new Error(`Unexpected RPC method: ${method}`)
    }
  }
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  return {
    rpc: createRpcClient({
      commands: { invoke },
      events,
      icons: createIconTransport(false),
    }),
    probes: () => calls.filter((call) => call === 'probe_route_egress').length,
    calls,
  }
}

function Scene({
  controls,
}: {
  controls: { setEditing: (value: boolean) => void }
}) {
  const [editing, setEditing] = useState(false)
  controls.setEditing = setEditing

  return (
    <DashboardProvider>
      <DndContext>
        <DndGridProvider
          value={{
            displayItems: items,
            getItemRect: () => ({ left: 0, top: 0, width: 320, height: 160 }),
            dropInfoMap: {},
            activeItemId: null,
            resizingItemId: null,
            disabled: !editing,
            sourceOnly: false,
            dragIdPrefix: '',
            isOverlay: false,
            constraintsMapRef: { current: {} },
            onResizeStart: () => {},
            onResizeMove: () => {},
            onResizeEnd: () => {},
          }}
        >
          <TooltipProvider>
            <NetworkInfoWidget id="network" />
          </TooltipProvider>
        </DndGridProvider>
      </DndContext>
    </DashboardProvider>
  )
}

async function mount(
  onTestFinished: (fn: () => void) => void,
  backend: Backend,
) {
  const harness = createRpcHarness(backend)
  const controls = { setEditing: (_value: boolean) => {} }
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchOnWindowFocus: false },
      mutations: { retry: false },
    },
  })
  const rootRoute = createRootRoute({ component: Outlet })
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/dashboard',
    component: () => <Scene controls={controls} />,
  })
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/settings/nyanpasu/public-ip',
    component: () => <div>Public IP settings</div>,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([dashboardRoute, settingsRoute]),
    history: createMemoryHistory({ initialEntries: ['/main/dashboard'] }),
  })
  const screen = await render(
    <TestQueryProvider client={queryClient} rpc={harness.rpc}>
      <RouterProvider router={router} />
    </TestQueryProvider>,
  )
  onTestFinished(async () => {
    await screen.unmount()
    queryClient.clear()
    harness.rpc.dispose()
  })
  return { ...harness, controls, router }
}

test('shows the current address, country, route and what answered', async ({
  onTestFinished,
}) => {
  const { probes, controls } = await mount(onTestFinished, {
    allowed: true,
    tun: false,
    hideAddresses: false,
  })

  await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
  await expect
    .element(page.getByText(regionName('JP')).first())
    .toBeInTheDocument()
  await expect
    .element(
      // A marquee repeats text that overflows.
      page.getByText(routeText('direct')).first(),
    )
    .toBeInTheDocument()

  expect(
    document.querySelector('[data-slot="network-info-address-swap"]'),
  ).not.toBeNull()

  expect(probes()).toBe(1)

  const refresh = page.getByRole('button', {
    name: m.dashboard_widget_network_info_refresh(),
  })
  await refresh.click()
  await expect.poll(probes).toBe(2)

  act(() => controls.setEditing(true))
  await expect.element(refresh).toBeDisabled()
})

test('nothing is probed while detection is off, and the notice links to its settings', async ({
  onTestFinished,
}) => {
  const { probes, router } = await mount(onTestFinished, {
    allowed: false,
    tun: false,
  })

  await expect
    .element(page.getByText(m.dashboard_widget_network_info_disabled()))
    .toBeInTheDocument()
  await expect
    .element(
      page.getByRole('button', {
        name: m.dashboard_widget_network_info_refresh(),
      }),
    )
    .toBeDisabled()
  expect(probes()).toBe(0)

  await page
    .getByRole('link', {
      name: m.dashboard_widget_network_info_open_settings(),
    })
    .click()
  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/settings/nyanpasu/public-ip')
})

test.for<[EgressMethod, Partial<Backend>]>([
  ['system_proxy', { systemProxy: true }],
  ['tun', { tun: true }],
  ['transparent_proxy', { localCapture: true }],
])(
  'a %s route is still probed and its method is named',
  async ([method, route], { onTestFinished }) => {
    const { probes } = await mount(onTestFinished, {
      allowed: true,
      tun: false,
      hideAddresses: false,
      ...route,
      probeImpl: async () => probed(method),
    })

    await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
    await expect
      .element(page.getByText(routeText(method)).first())
      .toBeInTheDocument()
    await expect
      .element(
        page.getByRole('button', {
          name: m.dashboard_widget_network_info_refresh(),
        }),
      )
      .toBeEnabled()
    expect(probes()).toBe(1)
  },
)

test('addresses are hidden by default and can be revealed without changing the preference', async ({
  onTestFinished,
}) => {
  const { calls } = await mount(onTestFinished, {
    allowed: true,
    tun: false,
  })

  const revealLabel = m.dashboard_widget_network_info_reveal_address()

  const spoiler = page.getByRole('button', { name: revealLabel })
  await expect.element(spoiler).toBeInTheDocument()
  const spoilerElement = spoiler.element()
  const spoilerText = spoilerElement.querySelector(
    '[data-slot="spoiler-text-content"]',
  )
  expect(spoilerElement).toHaveAttribute('data-revealed', 'false')
  expect(spoilerText).toHaveAttribute('aria-hidden', 'true')
  expect(getComputedStyle(spoilerText!).opacity).toBe('0')

  await spoiler.click()

  await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
  expect(spoilerElement).toHaveAttribute('data-revealed', 'true')
  expect(spoilerText).not.toHaveAttribute('aria-hidden')
  expect(calls.filter((call) => call === 'set_storage_item')).toHaveLength(0)
})

test('missing IPv6 and IPv4 each name the family that found nothing', async ({
  onTestFinished,
}) => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const intervals = vi.spyOn(window, 'setInterval')
  onTestFinished(() => {
    intervals.mockRestore()
    vi.useRealTimers()
  })

  await mount(onTestFinished, {
    allowed: true,
    tun: false,
    probeImpl: async () => ({ ...PROBED, ipv4: null, ipv6: null }),
  })

  await expect
    .element(page.getByText(m.dashboard_widget_network_info_no_ipv4()))
    .toBeInTheDocument()

  const delay = Number(intervals.mock.calls.at(-1)?.[1])
  expect(delay).toBeGreaterThanOrEqual(6000)
  await vi.advanceTimersByTimeAsync(delay + 1)
  await expect
    .element(page.getByText(m.dashboard_widget_network_info_no_ipv6()))
    .toBeInTheDocument()
})

test('shows loading state while probing and resolves to egress details', async ({
  onTestFinished,
}) => {
  let resolveProbe!: (value: RouteEgress) => void
  await mount(onTestFinished, {
    allowed: true,
    tun: false,
    hideAddresses: false,
    probeImpl: () =>
      new Promise<RouteEgress>((resolve) => {
        resolveProbe = resolve
      }),
  })

  await expect
    .element(page.getByText(m.dashboard_widget_network_info_probing()))
    .toBeInTheDocument()

  act(() => resolveProbe(PROBED))

  await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
})

test('shows error state when probing fails and allows retrying', async ({
  onTestFinished,
}) => {
  let shouldFail = true
  const { probes } = await mount(onTestFinished, {
    allowed: true,
    tun: false,
    hideAddresses: false,
    probeImpl: async () => {
      if (shouldFail) throw new Error('probe failed')
      return PROBED
    },
  })

  await expect
    .element(page.getByText(m.dashboard_widget_network_info_failed()))
    .toBeInTheDocument()
  expect(probes()).toBe(1)

  shouldFail = false
  await page
    .getByRole('button', {
      name: m.dashboard_widget_network_info_refresh(),
    })
    .first()
    .click()

  await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
  expect(probes()).toBe(2)
})
