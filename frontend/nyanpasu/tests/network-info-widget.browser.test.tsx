import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { act, useState } from 'react'
import { expect, test } from 'vitest'
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
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import type { DirectEgress } from '@nyanpasu/rpc/types'
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

const PROBED: DirectEgress = {
  kind: 'probed',
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
}

type Backend = {
  allowed: boolean
  tun: boolean
  hideAddresses?: boolean
  localCapture?: boolean
}

function createRpcHarness({
  allowed,
  tun,
  hideAddresses,
  localCapture,
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
      case 'get_app_config':
        return { enable_local_ip_probe: allowed } as T
      case 'get_clash_config':
        return {
          enable_tun_mode: tun,
          transparent_proxy: {
            mode: localCapture ? 'tproxy' : 'disabled',
            local: true,
          },
        } as T
      case 'probe_direct_egress':
        return PROBED as T
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
    }),
    probes: () => calls.filter((call) => call === 'probe_direct_egress').length,
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

test('each family shows its address, country and what answered', async ({
  onTestFinished,
}) => {
  const { probes, controls } = await mount(onTestFinished, {
    allowed: true,
    tun: false,
  })

  await expect.element(page.getByText('203.0.113.7')).toBeInTheDocument()
  await expect
    .element(
      page
        .getByText(
          `${regionName('JP')} · ${m.dashboard_widget_network_info_via_stun({
            server: 'stun.cloudflare.com:3478',
            transport: 'UDP',
          })}`,
          // A marquee repeats text that overflows.
        )
        .first(),
    )
    .toBeInTheDocument()
  await expect
    .element(page.getByText(m.dashboard_widget_network_info_no_address()))
    .toBeInTheDocument()
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

test('nothing is probed while TUN mode is on', async ({ onTestFinished }) => {
  const { probes } = await mount(onTestFinished, { allowed: true, tun: true })

  await expect
    .element(page.getByText(m.dashboard_widget_network_info_tun()))
    .toBeInTheDocument()
  expect(probes()).toBe(0)
})

test('local transparent capture suppresses the direct address probe', async ({
  onTestFinished,
}) => {
  const { probes } = await mount(onTestFinished, {
    allowed: true,
    tun: false,
    localCapture: true,
  })
  await expect
    .element(
      page.getByText(m.dashboard_widget_network_info_transparent_proxy()),
    )
    .toBeInTheDocument()
  await expect
    .element(
      page.getByRole('button', {
        name: m.dashboard_widget_network_info_refresh(),
      }),
    )
    .toBeDisabled()
  expect(probes()).toBe(0)
})

test('hidden addresses keep their shape but not their digits', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished, {
    allowed: true,
    tun: false,
    hideAddresses: true,
  })

  await expect.element(page.getByText('•••.•.•••.•')).toBeInTheDocument()
  await expect.element(page.getByText('203.0.113.7')).not.toBeInTheDocument()
})
