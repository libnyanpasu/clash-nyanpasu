import type { PropsWithChildren } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
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

test('renders the top three detail rows in stable speed order and hides configured targets', async ({
  onTestFinished,
}) => {
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
        hideTargets: true,
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
                  h: 3,
                },
              ],
              getItemRect: () => ({
                left: 0,
                top: 0,
                width: 360,
                height: 240,
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
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={queryClient}>
        <ClashWSProvider>
          <ConnectionDetailsBridge>
            <RouterProvider router={router} />
          </ConnectionDetailsBridge>
        </ClashWSProvider>
      </QueryClientProvider>
    </RpcProvider>,
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
    .poll(
      () =>
        card.querySelectorAll('[data-slot="widget-active-connections-list"] li')
          .length,
    )
    .toBe(3)
  const rows = [
    ...card.querySelectorAll('[data-slot="widget-active-connections-list"] li'),
  ].map((row) => row.textContent ?? '')
  expect(rows.map((row) => row.match(/worker-[a-z]\.exe/)?.[0])).toEqual([
    'worker-z.exe',
    'worker-a.exe',
    'worker-b.exe',
  ])
  expect(card.textContent).toContain(
    m.dashboard_widget_active_connections_hidden_target(),
  )
  expect(card.textContent).not.toContain('hidden-host-')
  expect(card.textContent).not.toContain('C:\\Applications\\Private App')
  expect(card.textContent).not.toContain('worker-c.exe')
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
})
