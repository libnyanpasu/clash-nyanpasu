import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { filesize } from 'filesize'
import {
  createElement,
  useState,
  type ComponentProps,
  type ReactNode,
} from 'react'
import { createRoot } from 'react-dom/client'
import { beforeEach, expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import type { WidgetComponentProps } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import {
  DEFAULT_WIDGET_CONFIGS,
  WidgetId,
} from '@/components/widgets/widget-config'
import { ProxyModeWidget } from '@/components/widgets/widget-proxy-mode'
import { SubscriptionQuotaWidget } from '@/components/widgets/widget-subscription-quota'
import { SubscriptionScheduleWidget } from '@/components/widgets/widget-subscription-schedule'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatDate } from '@/utils/date'
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
    pixelHeight?: number
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
                    height: options.pixelHeight ?? 220,
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

test('quota uses the profile title, remaining water level, and a full-card details link', async ({
  onTestFinished,
}) => {
  mount(onTestFinished, SubscriptionQuotaWidget)
  await expect
    .element(page.getByText('Remote plan', { exact: true }))
    .toBeVisible()
  await expect.element(page.getByText('70%')).toBeVisible()
  expect(
    document
      .querySelector('[data-slot="subscription-quota-wave"]')
      ?.getAttribute('data-percent'),
  ).toBe('70')
  const card = document.querySelector('[data-slot="subscription-quota-card"]')!
  const link = document.querySelector(
    '[data-slot="subscription-quota-details"]',
  )!
  expect(link.getBoundingClientRect().width).toBe(
    card.getBoundingClientRect().width,
  )
  expect(link.getBoundingClientRect().height).toBe(
    card.getBoundingClientRect().height,
  )
  const bounds = card.getBoundingClientRect()
  expect(
    document.elementFromPoint(bounds.left + 24, bounds.top + bounds.height / 2),
  ).toBe(link)
  expect(link.getAttribute('to')).toBe('/main/profiles/$type/detail/$uid')
  expect(
    document.querySelector(
      '[data-slot="subscription-quota-summary"] [data-slot="action-swap"]',
    ),
  ).not.toBeNull()
})

for (const { upload, download, percent } of [
  { upload: 0, download: 0, percent: 100 },
  { upload: 12_000_000, download: 0, percent: 0 },
]) {
  test(`quota renders the boundary water level ${percent} without a running wave`, async ({
    onTestFinished,
  }) => {
    hooks.useProfile.mockReturnValue({
      query: {
        ...profileQuery,
        data: {
          current: 'remote',
          items: [
            profile('remote', 'Boundary plan', {
              type: 'file',
              source: {
                type: 'remote',
                subscription: { upload, download, total: 10_000_000 },
              },
            }),
          ],
        },
      },
    })
    mount(onTestFinished, SubscriptionQuotaWidget)
    await expect.element(page.getByText(`${percent}%`)).toBeVisible()
    const wave = document.querySelector(
      '[data-slot="subscription-quota-wave"]',
    )!
    expect(wave.getAttribute('data-percent')).toBe(String(percent))
    expect(wave.getAnimations({ subtree: true })).toHaveLength(0)
  })
}

test('quota summary swaps to expiry and scrolls overflowing text', async ({
  onTestFinished,
}) => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const intervals = vi.spyOn(window, 'setInterval')
  onTestFinished(() => {
    intervals.mockRestore()
    vi.useRealTimers()
  })
  mount(onTestFinished, SubscriptionQuotaWidget)
  await expect.element(page.getByText('70%')).toBeVisible()
  const summary = document.querySelector<HTMLElement>(
    '[data-slot="subscription-quota-summary"]',
  )!
  summary.style.width = '100px'
  const delay = Number(intervals.mock.calls.at(-1)?.[1])
  expect(delay).toBeGreaterThanOrEqual(8000)
  await vi.advanceTimersByTimeAsync(delay + 1)
  await expect
    .poll(() => summary.textContent)
    .toContain(formatDate(1_900_000_000 * 1000))
  await expect
    .poll(
      () =>
        summary.querySelectorAll('[data-slot="text-marquee-content-item"]')
          .length,
    )
    .toBe(2)
  expect(summary.scrollWidth).toBe(summary.clientWidth)
})

test('quota warnings join the footer rotation while the profile title stays fixed', async ({
  onTestFinished,
}) => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  onTestFinished(() => {
    vi.useRealTimers()
  })
  cacheWidgetConfig({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
    quotaWarningPercent: 80,
  })
  mount(onTestFinished, SubscriptionQuotaWidget)
  const warning = m.dashboard_widget_subscription_quota_warning()
  await expect
    .element(page.getByText(warning, { exact: true }).first())
    .toBeVisible()
  const title = document.querySelector(
    '[data-slot="subscription-quota-title"]',
  )!
  const content = document.querySelector(
    '[data-slot="subscription-quota-content"]',
  )!
  const summary = document.querySelector(
    '[data-slot="subscription-quota-summary"]',
  )!
  expect(title.textContent).toBe('Remote plan')
  expect(title.querySelector('[data-slot="action-swap"]')).toBeNull()
  expect(summary.textContent).toContain(warning)
  expect(
    content.querySelectorAll('[data-slot="subscription-quota-summary"]'),
  ).toHaveLength(1)
  await expect
    .poll(
      async () => {
        await vi.advanceTimersByTimeAsync(1000)
        return summary.textContent?.includes(
          filesize(7_000_000, { standard: 'iec' }),
        )
      },
      { interval: 10, timeout: 5000 },
    )
    .toBe(true)
  await expect
    .element(page.getByText('Remote plan', { exact: true }))
    .toBeVisible()
  await expect
    .poll(
      async () => {
        await vi.advanceTimersByTimeAsync(1000)
        return summary.textContent?.includes(formatDate(1_900_000_000 * 1000))
      },
      { interval: 10, timeout: 5000 },
    )
    .toBe(true)
  await expect
    .poll(
      async () => {
        await vi.advanceTimersByTimeAsync(1000)
        return summary.textContent?.includes(warning)
      },
      { interval: 10, timeout: 5000 },
    )
    .toBe(true)
  expect(title.textContent).toBe('Remote plan')
  await expect.element(page.getByText('70%')).toBeVisible()
})

test('quota renders a straight waterline when animation is disabled', async ({
  onTestFinished,
}) => {
  cacheWidgetConfig({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
    waveStyle: 'double',
    animateWave: false,
  })
  mount(onTestFinished, SubscriptionQuotaWidget)
  await expect.element(page.getByText('70%')).toBeVisible()
  const wave = document.querySelector('[data-slot="subscription-quota-wave"]')!
  expect(wave.querySelectorAll('svg')).toHaveLength(1)
  expect(wave.getAnimations({ subtree: true })).toHaveLength(0)
  const outline = wave.querySelector<SVGPathElement>('path[fill="none"]')!
  expect(outline.getBBox().height).toBe(0)
})

test('quota checks an unconfirmed refresh from its header before allowing another update', async ({
  onTestFinished,
}) => {
  const refetch = vi
    .fn()
    .mockResolvedValueOnce({ isError: true, data: profileQuery.data })
    .mockResolvedValueOnce({ isError: true, data: profileQuery.data })
    .mockResolvedValueOnce({ isError: false, data: profileQuery.data })
  const update = vi
    .fn()
    .mockRejectedValueOnce(new MutationUnconfirmedError(new Error('timeout')))
    .mockResolvedValueOnce({ status: 'committed' })
  hooks.useProfile.mockReturnValue({ query: { ...profileQuery, refetch } })
  hooks.useProfileMutations.mockReturnValue({
    update: { isPending: false, mutateAsync: update },
  })
  mount(onTestFinished, SubscriptionQuotaWidget)
  await userEvent.click(
    page.getByRole('button', {
      name: m.dashboard_widget_subscription_quota_refresh(),
    }),
  )
  await expect
    .element(
      page.getByText(m.dashboard_widget_subscription_quota_unconfirmed()),
    )
    .toBeVisible()
  const check = page.getByRole('button', {
    name: m.dashboard_widget_operation_check(),
  })
  await userEvent.click(check)
  await expect
    .element(
      page.getByText(m.dashboard_widget_operation_check_failed()).first(),
    )
    .toBeVisible()
  expect(update).toHaveBeenCalledTimes(1)
  await userEvent.click(check)
  const refresh = page.getByRole('button', {
    name: m.dashboard_widget_subscription_quota_refresh(),
  })
  await expect.element(refresh).toBeEnabled()
  await userEvent.click(refresh)
  expect(update).toHaveBeenCalledTimes(2)
  await expect
    .poll(
      () =>
        document.querySelector('[data-slot="subscription-quota-content"]')
          ?.textContent,
    )
    .not.toContain(m.dashboard_widget_subscription_quota_unconfirmed())
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

test('schedule keeps the next update readable at its minimum height without scrolling', async ({
  onTestFinished,
}) => {
  mount(onTestFinished, SubscriptionScheduleWidget, {
    height: 2,
    pixelHeight: 160,
  })
  const expectedTime = formatDate('2026-01-01T02:00:00Z', 'HH:mm')
  await expect
    .element(page.getByText(expectedTime, { exact: true }))
    .toBeVisible()
  const content = document.querySelector<HTMLElement>(
    '[data-slot="subscription-schedule-content"]',
  )!
  expect(getComputedStyle(content).overflowY).toBe('hidden')
  expect(content.scrollHeight).toBeLessThanOrEqual(content.clientHeight)
  expect(content.querySelector('time')?.getAttribute('datetime')).toBe(
    '2026-01-01T02:00:00Z',
  )
})

test('schedule centers the profile and active state while syncing', async ({
  onTestFinished,
}) => {
  hooks.useProfileSyncStatus.mockReturnValue({
    data: { active: [{ ...finishedRun, state: { kind: 'running' } }] },
    isPending: false,
    isError: false,
    refetch: vi.fn(),
  })
  mount(onTestFinished, SubscriptionScheduleWidget, {
    height: 2,
    pixelHeight: 160,
  })
  await expect
    .element(
      page.getByText(m.dashboard_widget_subscription_schedule_running(), {
        exact: true,
      }),
    )
    .toBeVisible()
  const content = document.querySelector<HTMLElement>(
    '[data-slot="subscription-schedule-content"]',
  )!
  expect(
    content.querySelector('[data-slot="subscription-schedule-active"]'),
  ).not.toBeNull()
  expect(
    content.querySelector('[data-slot="subscription-schedule-next-run"]'),
  ).toBeNull()
  expect(
    content.querySelector('[data-slot="subscription-schedule-recent"]'),
  ).toBeNull()
  expect(content.scrollHeight).toBeLessThanOrEqual(content.clientHeight)
})

test('schedule swaps an error into the summary instead of stacking it below the profile', async ({
  onTestFinished,
}) => {
  const current = hooks.useProfileSyncStatus.getMockImplementation()!()
  hooks.useProfileSyncStatus.mockReturnValue({
    ...current,
    data: {
      ...current.data,
      registration_error: 'Unable to register the update schedule',
    },
  })
  mount(onTestFinished, SubscriptionScheduleWidget, {
    height: 2,
    pixelHeight: 160,
  })
  const error = m.dashboard_widget_subscription_schedule_registration_error({
    error: 'Unable to register the update schedule',
  })
  await expect.element(page.getByRole('alert', { name: error })).toBeVisible()
  const content = document.querySelector<HTMLElement>(
    '[data-slot="subscription-schedule-content"]',
  )!
  expect(
    content.querySelector(
      '[data-slot="subscription-schedule-summary"] [role="alert"]',
    ),
  ).not.toBeNull()
  expect(
    content.querySelector('[data-slot="subscription-schedule-next-run"]'),
  ).toBeNull()
  expect(content.scrollHeight).toBeLessThanOrEqual(content.clientHeight)
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
      page.getByRole('button', { name: m.dashboard_widget_proxy_mode_rule() }),
    )
    .toHaveAttribute('aria-pressed', 'false')
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
  const direct = page.getByRole('button', {
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
  const direct = page.getByRole('button', {
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

test('proxy mode selection follows the confirmed runtime value and premium exposes Script', async ({
  onTestFinished,
}) => {
  const upsert = vi.fn()
  hooks.useProxyMode.mockImplementation(function useModeFixture() {
    const [mode, setMode] = useState('rule')
    upsert.mockImplementation(async (next: string) => {
      setMode(next)
      return { status: 'committed' }
    })

    return {
      query: {
        data: { mode },
        isPending: false,
        isError: false,
        refetch: vi.fn(),
      },
      settingsQuery: {
        data: { core: 'clash' },
        isPending: false,
        isError: false,
      },
      isPending: false,
      upsert,
    }
  })
  mount(onTestFinished, ProxyModeWidget, { height: 2 })
  const rule = page.getByRole('button', {
    name: m.dashboard_widget_proxy_mode_rule(),
  })
  const script = page.getByRole('button', {
    name: m.dashboard_widget_proxy_mode_script(),
  })

  await expect.element(rule).toHaveAttribute('aria-pressed', 'true')
  await expect.element(script).toBeEnabled()
  await userEvent.click(script)
  await expect.element(script).toHaveAttribute('aria-pressed', 'true')
  await expect.element(rule).toHaveAttribute('aria-pressed', 'false')
  expect(upsert).toHaveBeenCalledWith('script')
  expect(
    document.querySelectorAll(
      '[data-slot="expressive-choice"][data-layout="flex"]',
    ),
  ).toHaveLength(1)
  expect(
    document.querySelector('[data-slot="widget-config-trigger"]'),
  ).toBeNull()
})
