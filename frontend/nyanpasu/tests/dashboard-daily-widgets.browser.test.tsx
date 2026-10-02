import { filesize } from 'filesize'
import { createElement, type ComponentProps, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { beforeEach, expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import type { WidgetComponentProps } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { ProfileShortcutsWidget } from '@/components/widgets/widget-profile-shortcuts'
import { ProxyModeWidget } from '@/components/widgets/widget-proxy-mode'
import { SubscriptionQuotaWidget } from '@/components/widgets/widget-subscription-quota'
import { SubscriptionScheduleWidget } from '@/components/widgets/widget-subscription-schedule'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { DndContext } from '@dnd-kit/core'
import { MutationUnconfirmedError } from '@nyanpasu/query'
import { RpcProvider } from '@nyanpasu/query/provider'
import type { ProfileItem_Serialize, RunDto } from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

const hooks = vi.hoisted(() => ({
  useProfile: vi.fn(),
  useProfileMutations: vi.fn(),
  useProfileSyncStatus: vi.fn(),
  useProfileSyncRuns: vi.fn(),
  useProxyMode: vi.fn(),
  useCoreStatus: vi.fn(),
}))

const backend = vi.hoisted(() => ({ value: new Map<string, string>() }))
const cacheWidgetConfig = (config: unknown) => {
  const serialized = JSON.stringify({
    version: 1,
    byInstance: { widget: config },
  })
  backend.value.set('dashboard-widget-configs', serialized)
  localStorage.setItem(
    `nyanpasu-kv-:${btoa('dashboard-widget-configs')}`,
    serialized,
  )
}

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const original = await importOriginal<typeof import('@nyanpasu/query')>()
  return {
    ...original,
    useProfile: hooks.useProfile,
    useProfileMutations: hooks.useProfileMutations,
    useProfileSyncStatus: hooks.useProfileSyncStatus,
    useProfileSyncRuns: hooks.useProfileSyncRuns,
    useProxyMode: hooks.useProxyMode,
    useCoreStatus: hooks.useCoreStatus,
  }
})

vi.mock('@/services/rpc', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/services/rpc')>()
  return {
    ...original,
    rpc: {
      ...original.rpc,
      getStorageItem: async (key: string) => ({
        status: 'ok',
        data: backend.value.get(key) ?? null,
      }),
      setStorageItem: async (key: string, value: string) => {
        backend.value.set(key, value)
        return { status: 'ok', data: null }
      },
      listenResync: () => () => {},
      events: {
        ...original.rpc.events,
        storageValueChangedEvent: { listen: async () => () => {} },
      },
    },
  }
})

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

const profile = (
  uid: string,
  name: string,
  definition: Record<string, unknown> = {
    type: 'file',
    source: { type: 'local', binding: { type: 'managed' } },
  },
): ProfileItem_Serialize =>
  ({ uid, name, type: 'config', config: definition }) as ProfileItem_Serialize

const remoteProfile = profile('remote', 'Remote plan', {
  type: 'file',
  source: {
    type: 'remote',
    url: 'https://example.test/profile.yaml',
    file: 'profiles/remote.yaml',
    option: {
      with_proxy: false,
      self_proxy: false,
      update_interval_minutes: 60,
    },
    updated_at: 1_800_000_000,
    subscription: {
      upload: 1_000_000,
      download: 2_000_000,
      total: 10_000_000,
      expire: 1_900_000_000,
    },
  },
})

const finishedRun = {
  id: 'run-1',
  job: 'profile-sync',
  definition_version: '1',
  admission_sequence: '1',
  trigger: 'manual',
  scheduled_at: null,
  admitted_at: '2026-01-01T00:00:00Z',
  finished_at: '2026-01-01T00:01:00Z',
  state: {
    kind: 'finished',
    completion: {
      outcome: { kind: 'succeeded' },
      output: { kind: 'none' },
      journal: { kind: 'durable' },
    },
  },
  last_log_sequence: '0',
  dropped_log_count: '0',
} as RunDto

const profileQuery = {
  data: { current: 'remote', items: [remoteProfile] },
  isPending: false,
  isError: false,
  error: null,
  refetch: vi.fn(async () => ({
    data: { current: 'remote', items: [remoteProfile] },
  })),
}

function setupHooks() {
  hooks.useProfile.mockReturnValue({
    query: profileQuery,
    update: {
      isPending: false,
      mutateAsync: vi.fn(async () => ({ status: 'committed' })),
    },
    activate: {
      isPending: false,
      mutateAsync: vi.fn(async () => ({ status: 'committed' })),
    },
  })
  hooks.useProfileMutations.mockReturnValue({
    update: {
      isPending: false,
      mutateAsync: vi.fn(async () => ({ status: 'committed' })),
    },
    activate: {
      isPending: false,
      mutateAsync: vi.fn(async () => ({ status: 'committed' })),
    },
  })
  hooks.useProfileSyncStatus.mockReturnValue({
    data: {
      scheduled: true,
      next_run_at: '2026-01-01T02:00:00Z',
      active: [],
      journal_degraded: false,
      registration_error: null,
      history_limit: 100,
    },
    isPending: false,
    isError: false,
    refetch: vi.fn(async () => ({})),
  })
  hooks.useProfileSyncRuns.mockReturnValue({
    data: { pages: [{ items: [finishedRun], next: null }] },
    isPending: false,
    isError: false,
    refetch: vi.fn(async () => ({})),
  })
  hooks.useProxyMode.mockReturnValue({
    query: {
      data: { mode: 'global' },
      isPending: false,
      isError: false,
      refetch: vi.fn(async () => ({ data: { mode: 'global' } })),
    },
    settingsQuery: {
      data: { core: 'mihomo' },
      isPending: false,
      isError: false,
    },
    isPending: false,
    upsert: vi.fn(async () => ({ status: 'committed' })),
  })
  hooks.useCoreStatus.mockReturnValue({
    data: { status: 'Running' },
    isPending: false,
    isError: false,
  })
}

beforeEach(() => {
  localStorage.clear()
  backend.value.clear()
  Object.values(hooks).forEach((hook) => hook.mockReset())
  setupHooks()
})

function mount(
  onTestFinished: (fn: () => void) => void,
  Widget: (props: WidgetComponentProps) => ReactNode,
  options: {
    id?: string
    width?: number
    height?: number
    sourceOnly?: boolean
    gridDisabled?: boolean
  } = {},
) {
  const id = options.id ?? 'widget'
  const item = {
    id,
    x: 0,
    y: 0,
    w: options.width ?? 4,
    h: options.height ?? 3,
  }
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  root.render(
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>
        <DashboardProvider>
          <TooltipProvider>
            <DndContext>
              <DndGridProvider
                value={{
                  displayItems: [item],
                  getItemRect: () => ({
                    left: 0,
                    top: 0,
                    width: 320,
                    height: 220,
                  }),
                  dropInfoMap: {},
                  activeItemId: null,
                  resizingItemId: null,
                  disabled: options.gridDisabled ?? true,
                  sourceOnly: options.sourceOnly ?? false,
                  dragIdPrefix: options.sourceOnly ? 'sheet:' : '',
                  isOverlay: false,
                  constraintsMapRef: { current: {} },
                  onResizeStart: () => {},
                  onResizeMove: () => {},
                  onResizeEnd: () => {},
                }}
              >
                <Widget id={id} onCloseClick={() => {}} />
              </DndGridProvider>
            </DndContext>
          </TooltipProvider>
        </DashboardProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )
  const cleanup = () => {
    root.unmount()
    client.clear()
    container.remove()
  }
  onTestFinished(cleanup)
  return cleanup
}

test('picker previews never mount profile, sync, or mode business hooks', async ({
  onTestFinished,
}) => {
  const cases = [
    [SubscriptionQuotaWidget, m.dashboard_widget_subscription_quota_title()],
    [
      SubscriptionScheduleWidget,
      m.dashboard_widget_subscription_schedule_title(),
    ],
    [ProfileShortcutsWidget, m.dashboard_widget_profile_shortcuts_title()],
    [ProxyModeWidget, m.dashboard_widget_proxy_mode_title()],
  ] as const

  for (const [Widget, title] of cases) {
    const cleanup = mount(onTestFinished, Widget, { sourceOnly: true })
    await expect.element(page.getByText(title).last()).toBeVisible()
    expect(hooks.useProfile).not.toHaveBeenCalled()
    expect(hooks.useProfileMutations).not.toHaveBeenCalled()
    expect(hooks.useProfileSyncStatus).not.toHaveBeenCalled()
    expect(hooks.useProfileSyncRuns).not.toHaveBeenCalled()
    expect(hooks.useProxyMode).not.toHaveBeenCalled()
    expect(hooks.useCoreStatus).not.toHaveBeenCalled()
    cleanup()
  }
})

test('quota display keeps the remaining amount when progress is hidden', async ({
  onTestFinished,
}) => {
  cacheWidgetConfig({
    type: WidgetId.SubscriptionQuota,
    target: { kind: 'current' },
    showExpiry: true,
    showProgress: false,
    expiryWarningDays: 7,
    quotaWarningPercent: 20,
  })
  mount(onTestFinished, SubscriptionQuotaWidget, {
    height: 2,
    gridDisabled: false,
  })
  await expect
    .element(page.getByText(m.dashboard_widget_subscription_quota_unknown()))
    .not.toBeInTheDocument()
  await expect
    .element(
      page.getByText(filesize(7_000_000, { standard: 'iec' }), { exact: true }),
    )
    .toBeVisible()
  expect(document.querySelector('a[aria-disabled="true"]')).not.toBeNull()
  expect(
    document.querySelector('[data-slot="subscription-quota-progress"]'),
  ).toBeNull()
})

test('quota refresh calls the profile update operation', async ({
  onTestFinished,
}) => {
  const update = vi.fn(async () => ({ status: 'committed' }))
  hooks.useProfileMutations.mockReturnValue({
    update: { isPending: false, mutateAsync: update },
  })
  mount(onTestFinished, SubscriptionQuotaWidget, { height: 3 })
  const refresh = page.getByRole('button', {
    name: m.dashboard_widget_subscription_quota_refresh(),
  })
  await expect.element(refresh).toBeEnabled()
  await userEvent.click(refresh)
  expect(update).toHaveBeenCalledWith({ uid: 'remote', option: null })
})

test('schedule reads a real next run and polls latest history at a low rate', async ({
  onTestFinished,
}) => {
  mount(onTestFinished, SubscriptionScheduleWidget, { gridDisabled: false })
  await expect
    .element(page.getByText(m.dashboard_widget_subscription_schedule_recent()))
    .toBeVisible()
  expect(hooks.useProfileSyncRuns).toHaveBeenCalledWith('remote', {
    refetchInterval: 60_000,
  })
  expect(hooks.useProfileSyncStatus).toHaveBeenCalledWith('remote')
  expect(document.querySelector('a[aria-disabled="true"]')).not.toBeNull()
})

test('schedule refresh starts one profile update', async ({
  onTestFinished,
}) => {
  const update = vi.fn(async () => ({ status: 'committed' }))
  hooks.useProfileMutations.mockReturnValue({
    update: { isPending: false, mutateAsync: update },
  })
  mount(onTestFinished, SubscriptionScheduleWidget)
  const refresh = page.getByRole('button', {
    name: m.dashboard_widget_subscription_schedule_refresh(),
  })
  await expect.element(refresh).toBeEnabled()
  await userEvent.click(refresh)
  expect(update).toHaveBeenCalledWith({ uid: 'remote', option: null })
})

test('profile switch uses favorites and is disabled while arranging widgets', async ({
  onTestFinished,
}) => {
  cacheWidgetConfig({
    type: WidgetId.ProfileShortcuts,
    profileUids: ['favorite'],
  })
  const favorite = profile('favorite', 'Favorite')
  hooks.useProfile.mockReturnValue({
    query: {
      ...profileQuery,
      data: { current: 'remote', items: [remoteProfile, favorite] },
    },
    activate: { isPending: false, mutateAsync: vi.fn() },
  })
  const arrangeCleanup = mount(onTestFinished, ProfileShortcutsWidget, {
    gridDisabled: false,
  })
  const select = page.getByRole('combobox')
  await expect.element(select).toBeDisabled()
  arrangeCleanup()
})

test('profile shortcut activates a selected favorite in normal viewing', async ({
  onTestFinished,
}) => {
  cacheWidgetConfig({
    type: WidgetId.ProfileShortcuts,
    profileUids: ['favorite'],
  })
  const favorite = profile('favorite', 'Favorite')
  const activate = vi.fn(async () => ({ status: 'committed' }))
  hooks.useProfile.mockReturnValue({
    query: {
      ...profileQuery,
      data: { current: 'remote', items: [remoteProfile, favorite] },
    },
    activate: { isPending: false, mutateAsync: activate },
  })
  mount(onTestFinished, ProfileShortcutsWidget)
  const select = page.getByRole('combobox')
  await expect.element(select).toBeEnabled()
  await userEvent.click(select)
  await userEvent.click(page.getByRole('option', { name: 'Favorite' }))
  expect(activate).toHaveBeenCalledWith('favorite')
})

test('empty favorites can choose any config without persisting a favorite', async ({
  onTestFinished,
}) => {
  const storedConfig = {
    type: WidgetId.ProfileShortcuts,
    profileUids: [],
  }
  cacheWidgetConfig(storedConfig)
  const favorite = profile('favorite', 'Favorite')
  hooks.useProfile.mockReturnValue({
    query: {
      ...profileQuery,
      data: { current: 'remote', items: [remoteProfile, favorite] },
    },
    activate: { isPending: false, mutateAsync: vi.fn() },
  })
  mount(onTestFinished, ProfileShortcutsWidget, { gridDisabled: true })
  await expect.element(page.getByRole('combobox')).toBeVisible()
  await userEvent.click(page.getByRole('combobox'))
  await expect
    .element(page.getByRole('option', { name: 'Favorite' }))
    .toBeVisible()
  await expect
    .element(page.getByRole('option', { name: 'Remote plan' }))
    .toBeVisible()
  expect(backend.value.get('dashboard-widget-configs')).toContain(
    '"profileUids":[]',
  )
})

test('proxy mode does not pretend an unknown runtime value is Rule', async ({
  onTestFinished,
}) => {
  hooks.useProxyMode.mockReturnValue({
    query: {
      data: { mode: 'unexpected' },
      isPending: false,
      isError: false,
      refetch: vi.fn(async () => ({ data: { mode: 'unexpected' } })),
    },
    settingsQuery: {
      data: { core: 'mihomo' },
      isPending: false,
      isError: false,
    },
    isPending: false,
    upsert: vi.fn(async () => ({ status: 'committed' })),
  })
  mount(onTestFinished, ProxyModeWidget, { height: 2 })
  await expect
    .element(page.getByText(m.dashboard_widget_proxy_mode_unknown()))
    .toBeVisible()
  await expect
    .element(
      page.getByRole('radio', { name: m.dashboard_widget_proxy_mode_rule() }),
    )
    .not.toBeChecked()
})

test('proxy mode applies a selected runtime mode', async ({
  onTestFinished,
}) => {
  const upsert = vi.fn(async () => ({ status: 'committed' }))
  hooks.useProxyMode.mockReturnValue({
    query: {
      data: { mode: 'global' },
      isPending: false,
      isError: false,
      refetch: vi.fn(async () => ({ data: { mode: 'global' } })),
    },
    settingsQuery: {
      data: { core: 'mihomo' },
      isPending: false,
      isError: false,
    },
    isPending: false,
    upsert,
  })
  mount(onTestFinished, ProxyModeWidget, { height: 2 })
  const direct = page.getByRole('radio', {
    name: m.dashboard_widget_proxy_mode_direct(),
  })
  await expect.element(direct).toBeEnabled()
  await userEvent.click(direct)
  expect(upsert).toHaveBeenCalledWith('direct')
})

test('an unconfirmed mode remains blocked until a successful explicit read', async ({
  onTestFinished,
}) => {
  const refetch = vi
    .fn()
    .mockResolvedValueOnce({ isError: true, data: { mode: 'global' } })
    .mockResolvedValueOnce({ isError: true, data: { mode: 'global' } })
    .mockResolvedValueOnce({ isError: false, data: { mode: 'global' } })
  const upsert = vi
    .fn()
    .mockRejectedValueOnce(new MutationUnconfirmedError(new Error('timeout')))
    .mockResolvedValueOnce({ status: 'committed' })
  hooks.useProxyMode.mockReturnValue({
    query: {
      data: { mode: 'global' },
      isPending: false,
      isError: false,
      refetch,
    },
    settingsQuery: {
      data: { core: 'mihomo' },
      isPending: false,
      isError: false,
    },
    isPending: false,
    upsert,
  })
  mount(onTestFinished, ProxyModeWidget)
  const direct = page.getByRole('radio', {
    name: m.dashboard_widget_proxy_mode_direct(),
  })
  await userEvent.click(direct)
  await expect
    .element(page.getByText(m.dashboard_widget_proxy_mode_unconfirmed()))
    .toBeVisible()
  await expect.element(direct).toBeDisabled()
  expect(upsert).toHaveBeenCalledTimes(1)

  await userEvent.click(
    page.getByRole('button', { name: m.dashboard_widget_operation_check() }),
  )
  await expect
    .element(page.getByText(m.dashboard_widget_operation_check_failed()))
    .toBeVisible()
  await expect.element(direct).toBeDisabled()
  expect(upsert).toHaveBeenCalledTimes(1)

  await userEvent.click(
    page.getByRole('button', { name: m.dashboard_widget_operation_check() }),
  )
  await expect.element(direct).toBeEnabled()
  await userEvent.click(direct)
  expect(upsert).toHaveBeenCalledTimes(2)
})
