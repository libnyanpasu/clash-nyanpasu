import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import type { PropsWithChildren } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { DashboardProvider } from '@/components/widgets/provider'
import { ActiveConnectionsWidget } from '@/components/widgets/widget-active-connections'
import { m } from '@/paraglide/messages'
import type { routeTree } from '@/route-tree.gen'
import { DndContext } from '@dnd-kit/core'
import { RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import type {
  ClashConnection_Serialize,
  ClashConnectionDetails_Serialize,
} from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { ClashConnectionDetailsProvider } from '../../query/src/provider/clash-connection-details-provider'
import {
  ClashWSProvider,
  useClashWSStatus,
} from '../../query/src/provider/clash-ws-provider'

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createRouter<typeof routeTree>>
  }
}

class FakeEventSource {
  static instances: FakeEventSource[] = []
  onopen: (() => void) | null = null
  onmessage: ((event: MessageEvent<string>) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  close = vi.fn()

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this)
  }
}

const connection = (
  id: string,
  downloadSpeed: number,
): ClashConnection_Serialize => ({
  id,
  metadata: {
    _extra: {},
    host: `hidden-host-${id}.example`,
    process: `C:\\Applications\\Private App\\worker-${id}.exe`,
  },
  upload: 0,
  download: 0,
  start: '2026-10-03T00:00:00Z',
  chains: [],
  rule: 'MATCH',
  rulePayload: '',
  _extra: {},
  downloadSpeed,
  uploadSpeed: 0,
})

const frame: ClashConnectionDetails_Serialize = {
  sequence: 1,
  connections: [
    connection('b', 2048),
    connection('z', 4096),
    connection('c', 1024),
    connection('a', 2048),
    connection('x', 512),
  ],
}

function ConnectionDetailsBridge({ children }: PropsWithChildren) {
  const { state } = useClashWSStatus()
  return (
    <ClashConnectionDetailsProvider connectorState={state}>
      {children}
    </ClashConnectionDetailsProvider>
  )
}

test.for([3, 2, 4])(
  'fits configured detail rows at grid height %i and hides configured targets',
  async (height, { onTestFinished }) => {
    FakeEventSource.instances = []
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
    vi.stubGlobal('EventSource', FakeEventSource)

    const invoked: string[] = []
    const storage = JSON.stringify({
      version: 1,
      byInstance: {
        'connections-instance': {
          type: 'active-connections',
          sort: 'download',
          topN: 5,
          showProcess: true,
          hideTargets: height !== 4,
        },
      },
    })
    const commands = {
      invoke: async <T,>(method: string, params?: Record<string, unknown>) => {
        invoked.push(method)
        if (method === 'get_storage_item')
          return (
            params?.key === 'dashboard-widget-configs' ? storage : null
          ) as T
        if (method === 'get_clash_ws_snapshot')
          return {
            sequence: 0,
            state: 'connected',
            recording: {
              connections: true,
              logs: false,
              traffic: true,
              memory: true,
            },
            connections: [],
            traffic: [],
            memory: [],
          } as T
        throw new Error(`Unexpected RPC command: ${method}`)
      },
    }
    const listenedEvents: string[] = []
    const events: RpcEventTransport = {
      listen: async (name) => {
        listenedEvents.push(name)
        return () => {}
      },
      once: async () => () => {},
      emit: async () => {},
      listenMutation: async () => () => {},
      listenResync: () => () => {},
      dispose: () => {},
    }
    const rpc = createRpcClient({ commands, events })
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const rootRoute = createRootRoute({ component: Outlet })
    const dashboardRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: '/main/dashboard',
      component: () => (
        <DashboardProvider>
          <DndContext>
            <DndGridProvider
              value={{
                displayItems: [
                  {
                    id: 'connections-instance',
                    x: 0,
                    y: 0,
                    w: 4,
                    h: height,
                  },
                ],
                getItemRect: () => ({
                  left: 0,
                  top: 0,
                  width: 360,
                  height: height === 2 ? 160 : height === 4 ? 320 : 240,
                }),
                dropInfoMap: {},
                activeItemId: null,
                resizingItemId: null,
                disabled: true,
                sourceOnly: false,
                dragIdPrefix: '',
                isOverlay: false,
                constraintsMapRef: { current: {} },
                onResizeStart: () => {},
                onResizeMove: () => {},
                onResizeEnd: () => {},
              }}
            >
              <ActiveConnectionsWidget id="connections-instance" />
            </DndGridProvider>
          </DndContext>
        </DashboardProvider>
      ),
    })
    const connectionsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: '/main/connections',
      component: () => <div>Connections page</div>,
    })
    const router = createRouter({
      routeTree: rootRoute.addChildren([dashboardRoute, connectionsRoute]),
      history: createMemoryHistory({ initialEntries: ['/main/dashboard'] }),
    })
    const screen = await render(
      <div style={{ transform: 'scale(0.5)', transformOrigin: 'top left' }}>
        <RpcProvider rpc={rpc}>
          <QueryClientProvider client={queryClient}>
            <ClashWSProvider>
              <ConnectionDetailsBridge>
                <TooltipProvider>
                  <RouterProvider router={router} />
                </TooltipProvider>
              </ConnectionDetailsBridge>
            </ClashWSProvider>
          </QueryClientProvider>
        </RpcProvider>
      </div>,
    )
    onTestFinished(async () => {
      await screen.unmount()
      await Promise.resolve()
      queryClient.clear()
      rpc.dispose()
      vi.unstubAllGlobals()
    })

    await expect.poll(() => FakeEventSource.instances.length).toBe(1)
    const detailsSource = FakeEventSource.instances[0]!
    expect(detailsSource.url).toBe('/bridge/connection-details')
    detailsSource.onmessage?.(
      new MessageEvent('message', { data: JSON.stringify(frame) }),
    )

    const card = screen.container.querySelector(
      '[data-slot="widget-active-connections-card"]',
    )!
    await expect
      .poll(() =>
        card.querySelector('[data-slot="widget-active-connections-list"]'),
      )
      .not.toBeNull()
    const list = card.querySelector(
      '[data-slot="widget-active-connections-list"]',
    )!
    // The first render shows one row until the list height has been measured.
    await expect
      .poll(() =>
        [...list.querySelectorAll('li')].map(
          (row) => row.textContent?.match(/worker-[a-z]\.exe/)?.[0],
        ),
      )
      .toEqual(
        height === 2
          ? ['worker-z.exe']
          : height === 4
            ? ['worker-z.exe', 'worker-a.exe', 'worker-b.exe']
            : ['worker-z.exe', 'worker-a.exe'],
      )
    expect(list.scrollHeight).toBeLessThanOrEqual(list.clientHeight)
    expect(getComputedStyle(list).overflowY).toBe('hidden')
    const remainder = card.querySelector(
      '[data-slot="widget-active-connections-remainder"]',
    )!
    expect(
      Math.abs(
        remainder.getBoundingClientRect().bottom -
          list.getBoundingClientRect().bottom,
      ),
    ).toBeLessThanOrEqual(1)
    expect(
      card.querySelector('[data-slot="widget-active-connections-count"]')
        ?.textContent,
    ).toBe('5')
    expect(
      card.querySelector('[data-slot="widget-active-connections-remainder"]')
        ?.textContent,
    ).toContain(
      m.dashboard_widget_active_connections_remaining({
        count: height === 2 ? 4 : height === 4 ? 2 : 3,
      }),
    )
    const firstLabel = card.querySelector(
      '[data-slot="widget-active-connections-label"]',
    )!
    expect(firstLabel.textContent).toBe(
      height === 4 ? 'hidden-host-z.example · worker-z.exe' : 'worker-z.exe',
    )
    if (height !== 4) expect(card.textContent).not.toContain('hidden-host-')
    expect(card.textContent).not.toContain('C:\\Applications\\Private App')
    expect(card.textContent).not.toContain('worker-x.exe')

    expect(
      listenedEvents.filter((name) => name === 'clash-ws-event'),
    ).toHaveLength(1)
    expect(
      invoked.filter((method) => method === 'get_clash_ws_snapshot'),
    ).toHaveLength(1)
    expect(
      FakeEventSource.instances.filter(
        (source) => source.url === '/bridge/connection-details',
      ),
    ).toHaveLength(1)
  },
)
