import { createRoot, type Root } from 'react-dom/client'
import { beforeEach, expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { WidgetId, type DashboardItem } from '@/components/widgets/consts'
import {
  DashboardProvider,
  useDashboardContext,
} from '@/components/widgets/provider'
import type { WidgetConfigStorage } from '@/components/widgets/widget-config'
import WidgetItem from '@/components/widgets/widget-item'
import { m } from '@/paraglide/messages'
import { DndContext } from '@dnd-kit/core'
import { RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import type { ProfileDocument_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

const backend = vi.hoisted(() => ({
  storage: null as string | null,
  writes: [] as string[],
  profiles: [] as unknown[],
}))

const profileData = [
  {
    uid: 'remote-uid',
    name: 'Remote subscription',
    type: 'config',
    config: {
      type: 'file',
      source: { type: 'remote', url: 'https://example.invalid/remote.yaml' },
    },
  },
  {
    uid: 'local-uid',
    name: 'Local config',
    type: 'config',
    config: { type: 'file', source: { type: 'local' } },
  },
  {
    uid: 'transform-uid',
    name: 'Remote transform',
    type: 'transform',
    transform: {
      source: { type: 'remote', url: 'https://example.invalid/patch.yaml' },
    },
  },
] as ProfileDocument_Serialize['items']

beforeEach(() => {
  localStorage.clear()
  backend.storage = null
  backend.writes = []
  backend.profiles = profileData
})

function createRpc() {
  const commands = {
    invoke: async <T,>(method: string, params?: Record<string, unknown>) => {
      switch (method) {
        case 'get_storage_item':
          return backend.storage as T
        case 'set_storage_item': {
          const value = params?.value as string
          backend.storage = value
          backend.writes.push(value)
          return null as T
        }
        case 'get_profiles':
          return { items: backend.profiles, valid: [], current: null } as T
        case 'clash_api_get_providers_proxies':
          return {
            providers: {
              SharedName: {
                name: 'SharedName',
                type: 'Proxy',
                proxies: [],
                vehicleType: 'HTTP',
              },
            },
          } as T
        case 'clash_api_get_providers_rules':
          return {
            providers: {
              SharedName: {
                name: 'SharedName',
                behavior: 'domain',
                format: 'yaml',
                ruleCount: 1,
                type: 'HTTP',
                updatedAt: null,
                vehicleType: 'HTTP',
              },
            },
          } as T
        default:
          throw new Error(`Unexpected RPC command: ${method}`)
      }
    },
  }
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenMutation: async () => () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  return { rpc: createRpcClient({ commands, events }), dispose: events.dispose }
}

function FixtureWidget({ id, type }: { id: string; type: WidgetId }) {
  return (
    <WidgetItem id={id} widgetType={type}>
      <div>{type}</div>
    </WidgetItem>
  )
}

function mount(
  onTestFinished: (fn: () => void) => void,
  items: DashboardItem[],
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root: Root = createRoot(container)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const { rpc, dispose: disposeEvents } = createRpc()
  const controls = { setEditing: (_value: boolean) => {}, loading: true }
  let disposed = false
  const dispose = () => {
    if (disposed) return
    disposed = true
    root.unmount()
    container.remove()
    queryClient.clear()
    disposeEvents()
  }
  const Scene = () => {
    const { setIsEditing, configLoading } = useDashboardContext()
    controls.setEditing = setIsEditing
    controls.loading = configLoading
    return (
      <DndGridProvider
        value={{
          displayItems: items,
          getItemRect: (item) => ({
            left: item.id === items[0]?.id ? 40 : 450,
            top: 40,
            width: 360,
            height: 180,
          }),
          dropInfoMap: {},
          activeItemId: null,
          resizingItemId: null,
          disabled: false,
          sourceOnly: false,
          dragIdPrefix: '',
          isOverlay: false,
          constraintsMapRef: { current: {} },
          onResizeStart: () => {},
          onResizeMove: () => {},
          onResizeEnd: () => {},
        }}
      >
        {items.map(({ id, type }) => (
          <FixtureWidget key={id} id={id} type={type} />
        ))}
      </DndGridProvider>
    )
  }
  root.render(
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={queryClient}>
        <DashboardProvider>
          <TooltipProvider>
            <DndContext>
              <Scene />
            </DndContext>
          </TooltipProvider>
        </DashboardProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )
  onTestFinished(dispose)
  return { controls, dispose }
}

const saved = () => JSON.parse(backend.storage!) as WidgetConfigStorage

function menuTrigger(index = 0) {
  return page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(index)
}

async function openMenu(index = 0) {
  await menuTrigger(index).click()
  return page.getByRole('dialog')
}

async function waitForReady(controls: { loading: boolean }) {
  await expect.poll(() => controls.loading).toBe(false)
}

const quotaItem: DashboardItem = {
  id: 'quota-instance',
  type: WidgetId.SubscriptionQuota,
  x: 0,
  y: 0,
  w: 4,
  h: 3,
}

const favoriteItem: DashboardItem = {
  id: 'favorite-instance',
  type: WidgetId.ProfileShortcuts,
  x: 0,
  y: 0,
  w: 4,
  h: 2,
}

const providerItem: DashboardItem = {
  id: 'provider-instance',
  type: WidgetId.ProviderUpdates,
  x: 0,
  y: 0,
  w: 4,
  h: 3,
}

const reportItems: DashboardItem[] = [
  {
    id: 'report-first',
    type: WidgetId.OriginTraffic,
    x: 0,
    y: 0,
    w: 4,
    h: 3,
  },
  {
    id: 'report-second',
    type: WidgetId.TargetTraffic,
    x: 4,
    y: 0,
    w: 4,
    h: 3,
  },
]

test('remote subscription target saves its UID and restores the selection after remount', async ({
  onTestFinished,
}) => {
  const first = mount(onTestFinished, [quotaItem])
  await waitForReady(first.controls)
  first.controls.setEditing(true)

  let dialog = await openMenu()
  await dialog
    .getByRole('combobox', { name: m.dashboard_widget_config_profile() })
    .click()
  await expect
    .element(
      page.getByRole('option', { name: 'Remote subscription', exact: true }),
    )
    .toBeVisible()
  expect(
    page.getByRole('option', { name: 'Local config', exact: true }).elements(),
  ).toHaveLength(0)
  expect(
    page
      .getByRole('option', { name: 'Remote transform', exact: true })
      .elements(),
  ).toHaveLength(0)
  await page
    .getByRole('option', { name: 'Remote subscription', exact: true })
    .click()

  await expect
    .poll(() => saved().byInstance['quota-instance'])
    .toMatchObject({
      target: { kind: 'fixed', profileUid: 'remote-uid' },
    })
  await userEvent.keyboard('{Escape}')

  first.dispose()
  const second = mount(onTestFinished, [quotaItem])
  await waitForReady(second.controls)
  second.controls.setEditing(true)
  dialog = await openMenu()
  expect(
    dialog
      .getByRole('combobox', {
        name: m.dashboard_widget_config_profile(),
      })
      .element().textContent,
  ).toContain('Remote subscription')
  expect(saved().byInstance['quota-instance']).toMatchObject({
    target: { kind: 'fixed', profileUid: 'remote-uid' },
  })
})

test('favorites can select config candidates in order and omit transforms', async ({
  onTestFinished,
}) => {
  const fixture = mount(onTestFinished, [favoriteItem])
  await waitForReady(fixture.controls)
  fixture.controls.setEditing(true)
  const dialog = await openMenu()

  await expect
    .element(dialog.getByRole('button', { name: 'Remote subscription' }))
    .toBeVisible()
  await expect
    .element(dialog.getByRole('button', { name: 'Local config' }))
    .toBeVisible()
  expect(
    dialog
      .getByRole('button', { name: 'Remote transform', exact: true })
      .elements(),
  ).toHaveLength(0)

  await dialog.getByRole('button', { name: 'Local config' }).click()
  await dialog.getByRole('button', { name: 'Remote subscription' }).click()
  await expect
    .poll(() => saved().byInstance['favorite-instance'])
    .toMatchObject({
      profileUids: ['local-uid', 'remote-uid'],
    })
  expect(
    (saved().byInstance['favorite-instance'] as { profileUids: string[] })
      .profileUids,
  ).not.toContain('transform-uid')
})

test('same-named proxy and rule provider references persist independently', async ({
  onTestFinished,
}) => {
  const fixture = mount(onTestFinished, [providerItem])
  await waitForReady(fixture.controls)
  fixture.controls.setEditing(true)
  let dialog = await openMenu()

  const proxy = dialog.getByRole('switch', {
    name: `${m.dashboard_widget_config_proxy_providers()}: SharedName`,
  })
  const rule = dialog.getByRole('switch', {
    name: `${m.dashboard_widget_config_rule_providers()}: SharedName`,
  })
  await expect.element(proxy).toBeVisible()
  await expect.element(rule).toBeVisible()
  await proxy.click()
  await rule.click()
  await expect
    .poll(() => saved().byInstance['provider-instance'])
    .toMatchObject({
      resources: [
        { kind: 'proxy', name: 'SharedName' },
        { kind: 'rule', name: 'SharedName' },
      ],
    })

  await userEvent.keyboard('{Escape}')
  dialog = await openMenu()
  await expect
    .element(
      dialog.getByRole('switch', {
        name: `${m.dashboard_widget_config_proxy_providers()}: SharedName`,
      }),
    )
    .toBeChecked()
  await expect
    .element(
      dialog.getByRole('switch', {
        name: `${m.dashboard_widget_config_rule_providers()}: SharedName`,
      }),
    )
    .toBeChecked()
})

test('report widgets save independent range and profile filters per instance', async ({
  onTestFinished,
}) => {
  const fixture = mount(onTestFinished, reportItems)
  await waitForReady(fixture.controls)
  fixture.controls.setEditing(true)

  let dialog = await openMenu(0)
  await dialog
    .getByRole('combobox', { name: m.dashboard_widget_config_range() })
    .click()
  await page
    .getByRole('option', { name: m.dashboard_widget_config_range_hour() })
    .click()
  await dialog
    .getByRole('combobox', { name: m.dashboard_widget_config_profile() })
    .click()
  await page.getByRole('option', { name: 'Remote subscription' }).click()
  await expect
    .poll(() => saved().byInstance['report-first'])
    .toMatchObject({ range: 'last_hour', profileUid: 'remote-uid' })
  await userEvent.keyboard('{Escape}')

  dialog = await openMenu(1)
  await dialog
    .getByRole('combobox', { name: m.dashboard_widget_config_range() })
    .click()
  await page
    .getByRole('option', { name: m.dashboard_widget_config_range_7days() })
    .click()
  await dialog
    .getByRole('combobox', { name: m.dashboard_widget_config_profile() })
    .click()
  await page.getByRole('option', { name: 'Local config' }).click()
  await expect
    .poll(() => saved().byInstance['report-second'])
    .toMatchObject({ range: 'last7_days', profileUid: 'local-uid' })

  expect(saved().byInstance['report-first']).toMatchObject({
    range: 'last_hour',
    profileUid: 'remote-uid',
  })
  expect(saved().byInstance['report-second']).toMatchObject({
    range: 'last7_days',
    profileUid: 'local-uid',
  })
  expect(backend.writes.length).toBeGreaterThanOrEqual(4)
})
