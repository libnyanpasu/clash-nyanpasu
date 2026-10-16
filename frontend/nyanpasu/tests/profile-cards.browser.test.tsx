import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import ProfilesHeader from '@/pages/(main)/main/profiles/$type/_modules/profiles-header'
import ProfilesList from '@/pages/(main)/main/profiles/$type/_modules/profiles-list'
import { m } from '@/paraglide/messages'
import { getLocale, overwriteGetLocale } from '@/paraglide/runtime'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})
vi.mock('@/pages/(main)/main/profiles/$type/index', () => ({
  Route: { useParams: () => ({ type: 'profile' }) },
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn(), ask: vi.fn() }))

vi.mock('@/components/providers/theme-provider', async () => {
  const { argbFromHex, themeFromSourceColor } =
    await import('@material/material-color-utilities')
  return {
    useExperimentalThemeContext: () => ({
      themePalette: themeFromSourceColor(argbFromHex('#1867C0')),
    }),
  }
})

const profile = (uid: string, remote: boolean): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: remote
    ? 'Flower_Trojan.yaml'
    : 'Local profile with a very long name to check narrow windows',
  config: {
    type: 'file',
    transforms: [],
    source: remote
      ? {
          type: 'remote',
          file: `${uid}.yaml`,
          url: 'https://example.com/sub',
          updated_at: 1780000000,
          option: {
            with_proxy: false,
            self_proxy: false,
            update_interval_minutes: 0,
          },
          subscription: {
            upload: 4 * 1024 ** 3,
            download: 60 * 1024 ** 3,
            total: 200 * 1024 ** 3,
            expire: 1800000000,
          },
        }
      : { type: 'local', binding: { type: 'managed', file: `${uid}.yaml` } },
  },
})

test.for(['en', 'zh-cn'] as const)(
  'profile cards in %s preserve subscription and activation behavior',
  async (locale, { onTestFinished }) => {
    await page.viewport(1200, 900)
    await document.fonts.ready
    vi.stubGlobal('isTauri', true)
    const previousGetLocale = getLocale
    overwriteGetLocale(() => locale)
    const items = [profile('local', false), profile('remote', true)]
    let current = 'remote'
    const updates: unknown[] = []
    const edits: string[] = []
    mockIPC((command, args) => {
      const wire = args as {
        method: string
        params: { uid: string; option: unknown }
      }
      const method = command === 'call_rpc' ? wire.method : command
      if (method === 'read_profile_file') {
        expect(wire.params.uid).toBe('local')
        return 'proxies: [{name: Node}]\nproxy-groups: [{name: Group}]\nrules: [MATCH,DIRECT]'
      }
      if (method === 'get_profiles')
        return { current, items, global_transforms: [], valid: [] }
      if (method === 'activate_profile') {
        current = wire.params.uid
        return { status: 'committed', value: null }
      }
      if (method === 'query_traffic_report') {
        expect(
          (
            wire.params as unknown as {
              request: { query: { filters: unknown[] } }
            }
          ).request.query.filters,
        ).toEqual([{ dimension: 'profile', value: 'local' }])
        return {
          total: {
            bytes: { upload: 1024 ** 3, download: 3 * 1024 ** 3 },
            connections: 12,
          },
          current_rate: null,
          rankings: [],
          topology: null,
        }
      }
      if (method === 'create_editor_window') {
        edits.push(wire.params.uid)
        return null
      }
      if (method === 'delete_clash_connections')
        return { status: 'committed', value: null }
      if (method === 'update_profile') {
        updates.push([wire.params.uid, wire.params.option])
        return { status: 'committed', value: null }
      }
      throw new Error(`Unexpected RPC ${method}`)
    })
    const queries = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    })
    const container = document.createElement('div')
    container.style.width = '960px'
    container.className = 'bg-mixed-background text-on-surface'
    document.body.append(container)
    const root = createRoot(container)
    const route = createRootRoute({
      component: () => (
        <TestQueryProvider client={queries}>
          <BlockTaskProvider>
            <TooltipProvider>
              <ContextMenuProvider>
                <ScrollArea>
                  <ProfilesHeader />
                  <ProfilesList className="p-4 pt-0" />
                </ScrollArea>
              </ContextMenuProvider>
            </TooltipProvider>
          </BlockTaskProvider>
        </TestQueryProvider>
      ),
    })
    const router = createRouter({
      routeTree: route,
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })
    root.render(<RouterProvider router={router} />)
    onTestFinished(() => {
      root.unmount()
      container.remove()
      queries.clear()
      vi.unstubAllGlobals()
      document.documentElement.classList.remove('dark')
      overwriteGetLocale(previousGetLocale)
    })
    const card = (uid: string) =>
      container.querySelector<HTMLElement>(`[data-profile-uid="${uid}"]`)!
    await expect
      .poll(
        () =>
          card('remote')?.querySelector('[data-slot="profile-card-remaining"]')
            ?.textContent,
      )
      .toBe('136 GiB')
    expect(
      card('remote')
        .querySelector('[role="meter"]')
        ?.getAttribute('aria-valuenow'),
    ).toBe('68')
    await expect
      .poll(
        () =>
          card('local')?.querySelector('[data-slot="profile-card-remaining"]')
            ?.textContent,
      )
      .toBe('4 GiB')
    expect(card('remote').getBoundingClientRect().width).toBeGreaterThan(
      card('local').getBoundingClientRect().width,
    )
    expect(card('remote').getBoundingClientRect().top).toBeLessThan(
      card('local').getBoundingClientRect().top,
    )
    await expect
      .poll(
        () =>
          card('local').querySelector(
            '[data-slot="profile-card-content-summary"]',
          )?.textContent,
      )
      .toContain(m.profile_card_nodes() + '1')
    expect(card('remote').getBoundingClientRect().height).toBe(288)
    expect(card('local').getBoundingClientRect().height).toBe(240)
    const trafficRows = Array.from(
      card('local').querySelector('[data-slot="profile-card-subscription"]')!
        .children,
    ).map((row) => row.getBoundingClientRect())
    expect(trafficRows).toHaveLength(3)
    expect(
      Math.abs(
        trafficRows[1].top -
          trafficRows[0].bottom -
          (trafficRows[2].top - trafficRows[1].bottom),
      ),
    ).toBeLessThanOrEqual(1)
    expect(card('local').textContent).not.toContain(
      m.dashboard_widget_traffic_report_download({ value: '' }),
    )
    expect(card('local').textContent).not.toContain(
      m.dashboard_widget_traffic_report_upload({ value: '' }),
    )
    expect(
      card('remote')
        .querySelector('[role="meter"]')
        ?.classList.contains('h-1.5'),
    ).toBe(true)
    const style = getComputedStyle(card('remote'))
    expect(style.borderTopLeftRadius).toBe(style.borderBottomLeftRadius)
    expect(card('remote').querySelector('canvas')).not.toBeNull()
    await page.viewport(1200, 650)
    expect(card('remote').getBoundingClientRect().bottom).toBeLessThanOrEqual(
      650,
    )
    const updateButton = Array.from(
      card('remote').querySelectorAll('button'),
    ).find(
      (button) =>
        button.getAttribute('aria-label') === m.profile_subscription_update(),
    )!
    expect(getComputedStyle(updateButton).display).toBe('grid')
    expect(updateButton.getBoundingClientRect().height).toBeLessThanOrEqual(44)
    updateButton.click()
    await expect.poll(() => updates).toEqual([['remote', null]])
    const editButton = card('remote').querySelector<HTMLButtonElement>(
      `button[aria-label="${m.profile_view_content_title()}"]`,
    )!
    editButton.click()
    await expect.poll(() => edits).toEqual(['remote'])
    await userEvent.click(card('remote'), { button: 'right' })
    await expect
      .element(page.getByRole('menuitem', { name: m.profile_delete_title() }))
      .toBeVisible()
    await userEvent.keyboard('{Escape}')
    const activateButton = Array.from(
      card('local').querySelectorAll('button'),
    ).find(
      (button) =>
        button.getAttribute('aria-label') === m.profile_active_title(),
    )!
    const detailsLink = card('local').querySelector<HTMLAnchorElement>(
      `a[aria-label="${m.profile_view_details_title()}"]`,
    )!
    expect(activateButton.getBoundingClientRect().left).toBeLessThan(
      detailsLink.getBoundingClientRect().left,
    )
    expect(activateButton.textContent).toContain(m.profile_active_title())
    for (const width of [390, 320]) {
      await page.viewport(width, 900)
      container.style.width = `${width}px`
      const summary = card('local').querySelector(
        '[data-slot="profile-card-content-summary"]',
      )!
      const footer = detailsLink.parentElement!.parentElement!
      expect(summary.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        footer.getBoundingClientRect().top,
      )
    }
    await page.viewport(1200, 650)
    container.style.width = '1200px'
    activateButton.click()
    await expect.poll(() => card('local').dataset.active).toBe('true')
    expect(card('local').getBoundingClientRect().height).toBe(288)
    expect(card('remote').getBoundingClientRect().height).toBe(240)
    expect(
      card('local').querySelector(
        `button[aria-label="${m.profile_active_title()}"]`,
      ),
    ).toBeNull()
    expect(
      card('local').querySelector(
        `button[aria-label="${m.profile_is_active_label()}"]`,
      ),
    ).toBeNull()
    expect(card('local').getBoundingClientRect().top).toBeLessThan(
      card('remote').getBoundingClientRect().top,
    )
    expect(card('local').querySelector('[role="meter"]')).toBeNull()
    for (const width of [390, 320]) {
      await page.viewport(width, 900)
      container.style.width = `${width}px`
      await expect.poll(() => container.scrollWidth).toBeLessThanOrEqual(width)
      const summary = card('local').querySelector(
        '[data-slot="profile-card-content-summary"]',
      )!
      const localFooter =
        card('local').querySelector('a')!.parentElement!.parentElement!
      expect(summary.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        localFooter.getBoundingClientRect().top,
      )
      const footer =
        card('remote').querySelector('a')!.parentElement!.parentElement!
      expect(footer.scrollWidth).toBeLessThanOrEqual(footer.clientWidth)
    }
    queries.setQueriesData(
      { queryKey: ['traffic-report'] },
      {
        total: { bytes: { upload: 0, download: 0 }, connections: 0 },
        current_rate: null,
        rankings: [],
        topology: null,
      },
    )
    await expect
      .poll(
        () =>
          card('local')?.querySelector('[data-slot="profile-card-remaining"]')
            ?.textContent,
      )
      .toBe('0 B')
    expect(card('local').textContent).not.toContain(
      m.topology_connection_count({ count: 0 }),
    )
    expect(card('local').textContent).not.toContain(
      m.dashboard_widget_traffic_report_upload({ value: '0 B' }),
    )
    expect(card('local').querySelector('[role="meter"]')).toBeNull()
    const unknown = profile('remote', true)
    if (
      unknown.type === 'config' &&
      unknown.config.type === 'file' &&
      unknown.config.source.type === 'remote'
    )
      unknown.config.source.subscription = {
        upload: null,
        download: null,
        total: null,
        expire: null,
      }
    queries.setQueryData(['getProfiles'], {
      current: 'remote',
      items: [unknown],
      global_transforms: [],
      valid: [],
    })
    await expect
      .poll(() => card('remote')?.textContent)
      .toContain(m.profile_card_quota_unknown())
    expect(card('remote').querySelector('[role="meter"]')).toBeNull()
  },
)

test('reorders profiles by dragging the whole card', async ({
  onTestFinished,
}) => {
  await page.viewport(1200, 900)
  vi.stubGlobal('isTauri', true)
  const items = ['first', 'second', 'third'].map((uid) => ({
    ...profile(uid, false),
    name: uid,
  }))
  const reorders: unknown[] = []
  mockIPC((command, args) => {
    const wire = args as { method: string; params: { list: string[] } }
    const method = command === 'call_rpc' ? wire.method : command
    if (method === 'get_profiles')
      return { current: null, items, global_transforms: [], valid: [] }
    if (method === 'read_profile_file') return 'proxies: []'
    if (method === 'query_traffic_report')
      return {
        total: { bytes: { upload: 0, download: 0 }, connections: 0 },
        current_rate: null,
        rankings: [],
        topology: null,
      }
    if (method === 'reorder_profiles_by_list') {
      reorders.push(wire.params.list)
      return { status: 'committed', value: null }
    }
    throw new Error(`Unexpected RPC ${method}`)
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const container = document.createElement('div')
  container.style.width = '960px'
  document.body.append(container)
  const root = createRoot(container)
  const route = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <ContextMenuProvider>
              <ScrollArea>
                <ProfilesList className="p-4 pt-0" />
              </ScrollArea>
            </ContextMenuProvider>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const router = createRouter({
    routeTree: route,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  root.render(<RouterProvider router={router} />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    vi.unstubAllGlobals()
  })

  const card = (uid: string) =>
    container.querySelector<HTMLElement>(`[data-profile-uid="${uid}"]`)
  await expect.poll(() => card('third')).not.toBeNull()

  // Grab the card body rather than any dedicated handle.
  const body = card('third')!.querySelector<HTMLElement>(
    '[data-slot="profile-card-title"]',
  )!
  await userEvent.dragAndDrop(body, card('first')!)

  await expect.poll(() => reorders).toEqual([['third', 'first', 'second']])
})
