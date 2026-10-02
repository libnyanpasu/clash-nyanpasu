import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { RENDER_MAP, type DashboardItem } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { DashboardTrafficProvider } from '@/components/widgets/widget-traffic-provider'
import { m } from '@/paraglide/messages'
import type { routeTree } from '@/route-tree.gen'
import { DndContext } from '@dnd-kit/core'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import type { ReportRequest, TrafficReport, Usage } from '@nyanpasu/rpc/types'
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

const reports = vi.hoisted(() => ({
  requests: [] as ReportRequest[],
  metadataReads: [] as string[],
  coreStopped: true,
}))

function usage(upload: number, download: number): Usage {
  return { bytes: { upload, download }, connections: 0 }
}

function reportFor(request: ReportRequest): TrafficReport {
  const totalBytes = request.query.range === 'last7_days' ? 7 * 1024 : 1024
  const value = usage(totalBytes / 2, totalBytes / 2)
  return {
    total: value,
    current_rate: null,
    topology: null,
    rankings: request.rankings.map((dimension) => ({
      dimension,
      distinct: 1,
      groups: [
        {
          key: dimension === 'origin' ? '/apps/example' : 'example',
          usage: value,
          current_rate: null,
        },
      ],
      other: usage(0, 0),
    })),
  }
}

function createRpc(storage: string, retention: string | null = '7d') {
  const invoke = async <T,>(
    method: string,
    params?: Record<string, unknown>,
  ) => {
    if (method === 'get_storage_item') {
      return (params?.key === 'dashboard-widget-configs' ? storage : null) as T
    }
    if (method === 'get_app_config') {
      reports.metadataReads.push(method)
      if (retention === null) throw new Error('settings unavailable')
      return { traffic_retention: retention } as T
    }
    if (method === 'get_profiles') {
      reports.metadataReads.push(method)
      return { items: [], valid: [], current: null } as T
    }
    if (method === 'query_traffic_report') {
      const request = (params as { request: ReportRequest }).request
      reports.requests.push(request)
      // Historical reports remain available even when the runtime core is stopped.
      expect(reports.coreStopped).toBe(true)
      return reportFor(request) as T
    }
    throw new Error(`Unexpected RPC command: ${method}`)
  }

  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenMutation: async () => () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }

  return createRpcClient({ commands: { invoke }, events })
}

const items: DashboardItem[] = [
  {
    id: 'recent-one',
    type: WidgetId.RecentTraffic,
    x: 0,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: 'origin-one',
    type: WidgetId.OriginTraffic,
    x: 3,
    y: 0,
    w: 3,
    h: 3,
  },
]

function Scene({
  dashboardItems,
  controls,
}: {
  dashboardItems: DashboardItem[]
  controls: { setEditing: (editing: boolean) => void }
}) {
  const [editing, updateEditing] = useState(false)
  controls.setEditing = updateEditing

  return (
    <DashboardProvider>
      <DndContext>
        <DndGridProvider
          value={{
            displayItems: dashboardItems,
            getItemRect: () => ({ left: 0, top: 0, width: 240, height: 240 }),
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
          <DashboardTrafficProvider items={dashboardItems}>
            <TooltipProvider>
              {dashboardItems.map(({ id, type }) => {
                const Widget = RENDER_MAP[type]
                return <Widget key={id} id={id} />
              })}
            </TooltipProvider>
          </DashboardTrafficProvider>
        </DndGridProvider>
      </DndContext>
    </DashboardProvider>
  )
}

function storedConfigs(
  originRange = 'last24_hours',
  profileUid: string | null = null,
) {
  return JSON.stringify({
    version: 1,
    byInstance: {
      'recent-one': {
        type: 'recent-traffic',
        range: 'last24_hours',
        profileUid: null,
        showDirections: false,
      },
      'origin-one': {
        type: 'origin-traffic',
        range: originRange,
        profileUid,
        showDirections: false,
        topN: 3,
        hideNames: false,
      },
    },
  })
}

async function mount(
  onTestFinished: (fn: () => void) => void,
  originRange = 'last24_hours',
  profileUid: string | null = null,
  retention: string | null = '7d',
  dashboardItems = items,
) {
  reports.requests = []
  reports.metadataReads = []
  reports.coreStopped = true
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const rpc = createRpc(storedConfigs(originRange, profileUid), retention)
  const controls = { setEditing: (_editing: boolean) => {} }
  const rootRoute = createRootRoute({ component: Outlet })
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/dashboard',
    component: () => (
      <Scene dashboardItems={dashboardItems} controls={controls} />
    ),
  })
  const topologyRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/topology',
    component: () => <div>Traffic topology route</div>,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([dashboardRoute, topologyRoute]),
    history: createMemoryHistory({ initialEntries: ['/main/dashboard'] }),
  })
  const rendered = await render(
    <TestQueryProvider client={queryClient} rpc={rpc}>
      <RouterProvider router={router} />
    </TestQueryProvider>,
  )
  onTestFinished(async () => {
    await rendered.unmount()
    queryClient.clear()
    rpc.dispose()
  })
  return { view: rendered, controls }
}

test('does not fetch traffic metadata when no report widgets are visible', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished, 'last24_hours', null, '7d', [])

  await expect.poll(() => reports.requests.length).toBe(0)
  expect(reports.metadataReads).toEqual([])
})

test('duplicate widgets share one report request, and stopped core still has history', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished)

  await expect.poll(() => reports.requests.length).toBe(1)
  await expect
    .poll(
      () =>
        view.container.querySelectorAll(
          '[data-slot="widget-traffic-report-card"]',
        ).length,
    )
    .toBe(2)
  expect(reports.requests[0].query.range).toBe('last24_hours')
  expect(reports.requests[0].rankings).toEqual([
    'origin',
    'exit',
    'target',
    'rule',
  ])
  await expect.poll(() => view.container.textContent).toContain('1.00 KiB')
})

test('report navigation is enabled in normal view and unavailable while editing', async ({
  onTestFinished,
}) => {
  const { view, controls } = await mount(onTestFinished)

  await expect.poll(() => reports.requests.length).toBe(1)
  const links = page.getByRole('link', {
    name: m.dashboard_widget_traffic_report_open(),
  })
  await expect.element(links.first()).toBeEnabled()
  expect(links.elements()).toHaveLength(2)

  const details = page.getByRole('button', {
    name: m.dashboard_widget_traffic_report_details(),
  })
  expect(details.elements()).toHaveLength(2)
  await expect.element(details.first()).toBeEnabled()

  controls.setEditing(true)
  await expect
    .poll(
      () =>
        view.container.querySelectorAll('[data-slot="widget-config-trigger"]')
          .length,
    )
    .toBe(2)
  expect(links.elements()).toHaveLength(0)
  expect(details.elements()).toHaveLength(2)
  await expect.element(details.first()).toBeEnabled()
})

test('different widget ranges use separate reports without crossing values', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, 'last7_days')

  await expect.poll(() => reports.requests.length).toBe(2)
  await expect
    .poll(() => view.container.textContent?.includes('7.00 KiB'))
    .toBe(true)
  expect(reports.requests.map((request) => request.query.range).sort()).toEqual(
    ['last24_hours', 'last7_days'],
  )
  expect(reports.coreStopped).toBe(true)
})

test('a saved profile filter keeps a visible missing UID after its profile is deleted', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    'last24_hours',
    'deleted-profile',
  )

  await expect.poll(() => reports.requests.length).toBe(2)
  await expect
    .poll(() => view.container.textContent)
    .toContain(
      m.dashboard_widget_config_missing_reference({ name: 'deleted-profile' }),
    )
  expect(
    reports.requests.some((request) =>
      request.query.filters.some(
        (filter) =>
          filter.dimension === 'profile' && filter.value === 'deleted-profile',
      ),
    ),
  ).toBe(true)
})

test('a window longer than retention is marked incomplete', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, 'last30_days')

  await expect.poll(() => reports.requests.length).toBe(2)
  await expect
    .poll(() => view.container.textContent)
    .toContain(m.dashboard_widget_traffic_report_retention_limited_short())
})

test('a failed retention read stays unknown instead of assuming seven days', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, 'last30_days', null, null)

  await expect.poll(() => reports.requests.length).toBe(2)
  await expect
    .poll(() => view.container.textContent)
    .toContain(m.dashboard_widget_traffic_report_retention_unknown())
  expect(view.container.textContent).not.toContain(
    m.dashboard_widget_traffic_report_retention_limited_short(),
  )
})
