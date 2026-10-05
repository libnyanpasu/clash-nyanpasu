import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { act, useState } from 'react'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import type { DashboardItem } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { ProviderUpdatesWidget } from '@/components/widgets/widget-provider-updates'
import { m } from '@/paraglide/messages'
import type { routeTree } from '@/route-tree.gen'
import { DndContext } from '@dnd-kit/core'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
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
  {
    id: 'providers',
    type: WidgetId.ProviderUpdates,
    x: 4,
    y: 0,
    w: 4,
    h: 3,
  },
]

function createRpcHarness() {
  const calls: string[] = []
  let failNextProxyRead = false
  const invoke = async <T,>(method: string) => {
    calls.push(method)
    switch (method) {
      case 'get_storage_item':
        return null as T
      case 'set_storage_item':
        return null as T
      case 'clash_api_get_providers_proxies':
        if (failNextProxyRead) {
          failNextProxyRead = false
          throw new Error('proxy provider read failed')
        }
        return {
          'Proxy A': {
            name: 'Proxy A',
            type: 'Proxy',
            vehicleType: 'HTTP',
            updatedAt: null,
            subscriptionInfo: null,
            proxies: [],
          },
        } as T
      case 'clash_api_get_providers_rules':
        return {
          providers: {
            'Rule A': {
              name: 'Rule A',
              behavior: 'domain',
              format: 'yaml',
              ruleCount: 1,
              type: 'HTTP',
              updatedAt: null,
              vehicleType: 'HTTP',
            },
          },
        } as T
      case 'update_proxy_provider':
        throw new Error('provider update reply lost')
      default:
        throw new Error(`Unexpected RPC method: ${method}`)
    }
  }
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenMutation: async () => () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  return {
    rpc: createRpcClient({
      commands: {
        invoke: async <T,>(method: string, _params?: Record<string, unknown>) =>
          invoke<T>(method),
      },
      events,
    }),
    calls,
    failProxyRead: () => {
      failNextProxyRead = true
    },
    dispose: events.dispose,
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
            getItemRect: (item) => ({
              left: item.x * 80,
              top: item.y * 80,
              width: 320,
              height: 240,
            }),
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
            <ProviderUpdatesWidget id="providers" />
          </TooltipProvider>
        </DndGridProvider>
      </DndContext>
    </DashboardProvider>
  )
}

async function mount(onTestFinished: (fn: () => void) => void) {
  const harness = createRpcHarness()
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
  const debugRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/settings/debug',
    component: () => <div>Debug settings</div>,
  })
  const providersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/providers',
    component: () => <div>Providers</div>,
  })
  const proxyProviderRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/providers/proxies/$key',
    component: () => <div>Proxy provider</div>,
  })
  const ruleProviderRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/providers/rules/$key',
    component: () => <div>Rule provider</div>,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      dashboardRoute,
      debugRoute,
      providersRoute,
      proxyProviderRoute,
      ruleProviderRoute,
    ]),
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
  return { ...harness, controls, screen }
}

test('status widget actions work in view mode and are disabled while editing', async ({
  onTestFinished,
}) => {
  const { calls, controls } = await mount(onTestFinished)
  const providerRefresh = page.getByRole('button', {
    name: m.dashboard_widget_provider_updates_refresh({ name: 'Proxy A' }),
  })

  await expect
    .poll(() => calls.includes('clash_api_get_providers_proxies'))
    .toBe(true)
  await expect.element(providerRefresh).toBeEnabled()

  act(() => controls.setEditing(true))
  await expect.element(providerRefresh).toBeDisabled()
})

test('provider rows fit the available height without an inner scrollbar', async ({
  onTestFinished,
}) => {
  const { screen } = await mount(onTestFinished)
  const card = screen.container.querySelector(
    '[data-slot="widget-provider-updates-card"]',
  )!
  const list = card.querySelector('[data-slot="widget-provider-updates-list"]')!

  await expect.poll(() => list.querySelectorAll('li').length).toBe(2)
  expect(list.scrollHeight).toBeLessThanOrEqual(list.clientHeight)
  expect(list.querySelector('ul')?.className).not.toMatch(
    /overflow-(auto|y-auto)/,
  )
})

test('provider refresh requires a successful read-only check before another update', async ({
  onTestFinished,
}) => {
  const harness = await mount(onTestFinished)
  const refreshName = m.dashboard_widget_provider_updates_refresh({
    name: 'Proxy A',
  })
  const checkName = m.dashboard_widget_provider_updates_check({
    name: 'Proxy A',
  })
  const updateCount = () =>
    harness.calls.filter((call) => call === 'update_proxy_provider').length
  const proxyReadCount = () =>
    harness.calls.filter((call) => call === 'clash_api_get_providers_proxies')
      .length

  const refresh = page.getByRole('button', { name: refreshName })
  await expect.element(refresh).toBeEnabled()
  await userEvent.click(refresh)
  await expect.poll(updateCount).toBe(1)
  const check = page.getByRole('button', { name: checkName })
  await expect.element(check).toBeVisible()

  const readsBeforeCheck = proxyReadCount()
  harness.failProxyRead()
  await userEvent.click(check)
  await expect.poll(proxyReadCount).toBe(readsBeforeCheck + 1)
  await expect.element(check).toBeEnabled()
  expect(updateCount()).toBe(1)

  await userEvent.click(check)
  await expect
    .element(page.getByRole('button', { name: refreshName }))
    .toBeEnabled()
  expect(updateCount()).toBe(1)
})
