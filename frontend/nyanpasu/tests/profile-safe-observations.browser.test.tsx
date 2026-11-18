import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { createElement, type ComponentProps, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { WidgetId } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetDataCalibration } from '@/components/widgets/widget-data-calibration'
import { SubscriptionQuotaWidget } from '@/components/widgets/widget-subscription-quota'
import { SubscriptionScheduleWidget } from '@/components/widgets/widget-subscription-schedule'
import ProxiesLayoutProvider from '@/pages/(main)/main/proxies/_modules/proxies-layout-provider'
import { useGroupLayout } from '@/pages/(main)/main/proxies/_modules/use-group-layout'
import { m } from '@/paraglide/messages'
import { DndContext } from '@dnd-kit/core'
import {
  KvStorageProvider,
  MutationProvider,
  RpcProvider,
} from '@nyanpasu/query/provider'
import {
  createIconTransport,
  createRpcClient,
  type RpcEventTransport,
  type StateChanged,
} from '@nyanpasu/rpc'
import type {
  ProfilesListing_Serialize,
  ProfileSyncSummary,
  Proxies_Serialize,
  ProxyGroupLayout,
  SyncRunPageSummary,
} from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

vi.mock('@tanstack/react-router', async (importOriginal) => {
  const original =
    await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...original,
    Link: ({
      children,
      ...props
    }: ComponentProps<'a'> & { children: ReactNode }) =>
      createElement('a', props, children),
  }
})

// Ordinary components and providers must read only the safe summary queries,
// never full profile documents or full sync/log history.
const FORBIDDEN_READS = new Set([
  'get_profiles',
  'get_profile_sync_status',
  'get_profile_sync_runs',
  'get_profile_sync_logs',
])

const baseListing = (name = 'Safe Remote'): ProfilesListing_Serialize => ({
  current: 'remote',
  global_transforms: [],
  folders: [],
  proxy_group_layouts: {
    remote: {
      folders: [],
      ungrouped: ['Bravo', 'Alpha'],
    },
  },
  items: [
    {
      uid: 'remote',
      name,
      custom_name: true,
      has_desc: true,
      type: 'config',
      config: {
        type: 'file',
        transforms: [],
        source: {
          type: 'remote',
          updated_at: 1_800_000_000,
          option: {
            has_user_agent: true,
            with_proxy: false,
            self_proxy: false,
            update_interval_minutes: 60,
          },
          subscription: {
            upload: 2_000_000,
            download: 1_000_000,
            total: 10_000_000,
            expire: 1_900_000_000,
          },
        },
      },
    },
  ],
})

const proxiesFixture: Proxies_Serialize = {
  global: null,
  groups: [
    {
      name: 'Alpha',
      type: 'Selector',
      all: ['hk-1'],
      now: 'hk-1',
      fixed: null,
      delayKey: '',
      expectedStatus: null,
      hidden: false,
      iconConfigured: false,
      capabilities: { select: true, clearFixed: false },
    },
    {
      name: 'Bravo',
      type: 'URLTest',
      all: ['jp-1'],
      now: 'jp-1',
      fixed: null,
      delayKey: '',
      expectedStatus: null,
      hidden: false,
      iconConfigured: false,
      capabilities: { select: true, clearFixed: false },
    },
  ],
  nodes: {},
}

function createHarness() {
  const listeners = new Map<
    string,
    Set<(event: { event: string; id: number; payload: unknown }) => void>
  >()
  const called: string[] = []
  let listing = baseListing()
  let syncSummary: ProfileSyncSummary = {
    scheduled: true,
    next_run_at: '2026-01-01T02:00:00Z',
    active: [],
    journal_degraded: false,
    has_registration_error: true,
    history_limit: 50,
  }
  const runSummary: SyncRunPageSummary = {
    items: [
      {
        id: 'run-1',
        admission_sequence: '1',
        trigger: 'manual',
        scheduled_at: null,
        admitted_at: '2026-01-01T00:00:00Z',
        finished_at: '2026-01-01T00:01:00Z',
        state: {
          kind: 'finished',
          outcome: 'succeeded',
          journal_degraded: false,
        },
      },
    ],
    next: null,
  }
  const groupLayoutCommits: ProxyGroupLayout[] = []

  const invoke = vi.fn(
    async (method: string, params?: Record<string, unknown>) => {
      called.push(method)
      if (FORBIDDEN_READS.has(method)) {
        throw new Error(
          `Forbidden full profile read on ordinary mount: ${method}`,
        )
      }
      switch (method) {
        case 'get_profiles_listing':
          return listing
        case 'get_profile_sync_status_summary':
          return syncSummary
        case 'get_profile_sync_runs_summary':
          return runSummary
        case 'get_proxies':
          return proxiesFixture
        case 'set_proxy_group_layout': {
          const layout = params?.layout as ProxyGroupLayout
          groupLayoutCommits.push(layout)
          listing = {
            ...listing,
            proxy_group_layouts: { [String(params?.profile)]: layout },
          }
          return { status: 'committed', value: null, commits: [] }
        }
        case 'get_storage_item':
          return { value: null, revision: 0 }
        default:
          return null
      }
    },
  )

  const events: RpcEventTransport = {
    listen: async (name, callback) => {
      let set = listeners.get(name)
      if (!set) listeners.set(name, (set = new Set()))
      set.add(
        callback as (event: {
          event: string
          id: number
          payload: unknown
        }) => void,
      )
      return () => set?.delete(callback as never)
    },
    once: async () => () => {},
    emit: async (name, payload) => {
      for (const callback of [...(listeners.get(name) ?? [])]) {
        callback({ event: name, id: 1, payload })
      }
    },
    listenResync: () => () => {},
    dispose: () => listeners.clear(),
  }

  const rpc = createRpcClient({
    icons: createIconTransport(false),
    commands: {
      invoke: async <T,>(method: string, params?: Record<string, unknown>) =>
        (await invoke(method, params)) as T,
    },
    events,
  })

  return {
    rpc,
    called,
    groupLayoutCommits,
    setListing: (next: ProfilesListing_Serialize) => {
      listing = next
    },
    setSyncSummary: (next: ProfileSyncSummary) => {
      syncSummary = next
    },
    emitStateChanged: (payload: StateChanged) =>
      rpc.events.stateChangedEvent.emit(payload),
  }
}

test('mounted dashboard quota, schedule and calibration widgets use only safe summary RPCs and react to profile events', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  const harness = createHarness()
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    client.clear()
    harness.rpc.dispose()
  })

  const items = [
    { id: 'quota', type: WidgetId.SubscriptionQuota, x: 0, y: 0, w: 4, h: 2 },
    {
      id: 'schedule',
      type: WidgetId.SubscriptionSchedule,
      x: 4,
      y: 0,
      w: 4,
      h: 2,
    },
  ]

  root.render(
    <RpcProvider rpc={harness.rpc}>
      <QueryClientProvider client={client}>
        <MutationProvider>
          <KvStorageProvider>
            <DashboardProvider>
              <TooltipProvider>
                <DndContext>
                  <DndGridProvider
                    value={{
                      displayItems: items,
                      getItemRect: () => ({
                        left: 0,
                        top: 0,
                        width: 320,
                        height: 160,
                      }),
                      dropInfoMap: {},
                      activeItemId: null,
                      resizingItemId: null,
                      sourceOnly: false,
                      dragIdPrefix: '',
                      isOverlay: false,
                      disabled: false,
                      constraintsMapRef: { current: {} },
                      onResizeStart: () => {},
                      onResizeMove: () => {},
                      onResizeEnd: () => {},
                    }}
                  >
                    <WidgetDataCalibration items={items} />
                    <SubscriptionQuotaWidget
                      id="quota"
                      onCloseClick={() => {}}
                    />
                    <SubscriptionScheduleWidget
                      id="schedule"
                      onCloseClick={() => {}}
                    />
                  </DndGridProvider>
                </DndContext>
              </TooltipProvider>
            </DashboardProvider>
          </KvStorageProvider>
        </MutationProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )

  await expect.element(page.getByText('70%')).toBeVisible()
  await expect
    .element(
      page.getByRole('alert', {
        name: m.dashboard_widget_subscription_schedule_registration_failed(),
      }),
    )
    .toBeVisible()

  expect(harness.called).toContain('get_profiles_listing')
  expect(harness.called).toContain('get_profile_sync_status_summary')
  expect(harness.called).toContain('get_profile_sync_runs_summary')
  for (const forbidden of FORBIDDEN_READS) {
    expect(harness.called).not.toContain(forbidden)
  }

  harness.setListing(baseListing('Renamed Safe Remote'))
  await harness.emitStateChanged({ kind: 'profiles', files: [] })
  await expect
    .element(page.getByText('Renamed Safe Remote').first())
    .toBeVisible()
})

function ProxiesLayoutProbe() {
  const { layout, moveGroup } = useGroupLayout()
  return (
    <div>
      <span data-slot="group-order">
        {layout?.ungrouped.join(',') ?? 'loading'}
      </span>
      <button type="button" onClick={() => moveGroup('Alpha', null, 0)}>
        Move Alpha first
      </button>
    </div>
  )
}

test('mounted ProxiesLayoutProvider reads safe listing and updates proxy group layout without calling get_profiles', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  const harness = createHarness()
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    client.clear()
    harness.rpc.dispose()
  })

  root.render(
    <RpcProvider rpc={harness.rpc}>
      <QueryClientProvider client={client}>
        <MutationProvider>
          <KvStorageProvider>
            <ProxiesLayoutProvider>
              <ProxiesLayoutProbe />
            </ProxiesLayoutProvider>
          </KvStorageProvider>
        </MutationProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )

  await expect.element(page.getByText('Bravo,Alpha')).toBeVisible()
  await userEvent.click(page.getByRole('button', { name: 'Move Alpha first' }))

  await expect.poll(() => harness.groupLayoutCommits.length).toBe(1)
  expect(harness.groupLayoutCommits[0]).toEqual({
    folders: [],
    ungrouped: ['Alpha', 'Bravo'],
  })
  await expect.element(page.getByText('Alpha,Bravo')).toBeVisible()
  expect(harness.called).toContain('get_profiles_listing')
  expect(harness.called).not.toContain('get_profiles')
})
