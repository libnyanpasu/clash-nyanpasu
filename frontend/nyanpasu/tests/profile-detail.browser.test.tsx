import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page } from 'vitest/browser'
import { SharedTooltip } from '@nyanpasu/ui/shared-tooltip'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import { Route } from '@/pages/(main)/main/profiles/$type/detail/$uid'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import { ask } from '@/utils/notification'
import type { ProfileItem_Serialize, RunDto } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet as RouterOutlet,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})
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
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

const remote: ProfileItem_Serialize = {
  uid: 'remote',
  name: 'Flower_Trojan.yaml',
  type: 'config',
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'remote',
      file: 'remote.yaml',
      url: 'https://example.test/sub',
      option: {
        with_proxy: false,
        self_proxy: false,
        update_interval_minutes: 60,
      },
      subscription: { upload: 10, download: 10, total: 100 },
    },
  },
}

const transform: ProfileItem_Serialize = {
  uid: 'fix',
  name: 'Fix rules',
  type: 'transform',
  transform: {
    type: 'script',
    runtime: 'javascript',
    source: { type: 'local', binding: { type: 'managed', file: 'fix.js' } },
  },
}

const other: ProfileItem_Serialize = {
  ...transform,
  uid: 'other',
  name: 'Other rules',
}

const host: ProfileItem_Serialize = {
  uid: 'host',
  name: 'Host profile',
  type: 'config',
  config: {
    type: 'file',
    transforms: ['fix'],
    source: { type: 'local', binding: { type: 'managed', file: 'host.yaml' } },
  },
}

const chained: ProfileItem_Serialize = {
  ...remote,
  config: { ...remote.config, transforms: ['fix'] },
} as ProfileItem_Serialize

const succeeded = (sequence: number): RunDto => ({
  id: `run-${sequence}`,
  job: 'profiles/sync/remote',
  definition_version: '1',
  admission_sequence: String(sequence),
  trigger: 'scheduled',
  scheduled_at: null,
  admitted_at: '2026-09-30T00:00:00Z',
  finished_at: '2026-09-30T00:00:01Z',
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
})

const deferred = () => {
  const gate: { open?: (value: unknown) => void } = {}
  const promise = new Promise((resolve) => (gate.open = resolve))
  return { promise, resolve: (value: unknown) => gate.open?.(value) }
}

const mountDetail = async ({
  items,
  initialEntries,
  initialIndex,
  globalTransforms = [],
  runs = [],
  nextRunAt = null,
  width = 1000,
  rpc,
  onTestFinished,
}: {
  items: ProfileItem_Serialize[]
  globalTransforms?: string[]
  runs?: RunDto[]
  nextRunAt?: string | null
  /** The width of the content area, which decides the panel layout. */
  width?: number
  initialEntries: string[]
  initialIndex?: number
  rpc?: (method: string, params: Record<string, unknown>) => unknown
  onTestFinished: (fn: () => void) => void
}) => {
  await page.viewport(width, 900)
  vi.stubGlobal('isTauri', true)
  const calls: string[] = []
  mockIPC((command, args) => {
    const wire = args as { method: string; params: Record<string, unknown> }
    const method = command === 'call_rpc' ? wire.method : command
    calls.push(method)
    if (method === 'get_profiles')
      return {
        current: 'other',
        items,
        global_transforms: globalTransforms,
        valid: [],
      }
    if (method === 'get_profile_sync_status')
      return {
        scheduled: false,
        next_run_at: nextRunAt,
        active: [],
        journal_degraded: false,
        registration_error: null,
        history_limit: 10,
      }
    if (method === 'get_profile_sync_runs') return { items: runs, next: null }
    if (method === 'read_profile_file') return 'proxies: [{name: Node}]'
    const result = rpc?.(method, wire.params)
    if (result !== undefined) return result
    throw new Error(`Unexpected RPC ${method}`)
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const root = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <SharedTooltip>
              <SnackbarProvider>
                <ImportUrlProvider>
                  <ProfilesLayoutProvider>
                    <RouterOutlet />
                  </ProfilesLayoutProvider>
                </ImportUrlProvider>
              </SnackbarProvider>
            </SharedTooltip>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const detail = Route.update({
    id: '/(main)/main/profiles/$type/detail/$uid',
    path: '/main/profiles/$type/detail/$uid',
    getParentRoute: () => root,
  } as never)
  const list = createRoute({
    getParentRoute: () => root,
    path: '/main/profiles/$type',
    component: () => <p>profile list</p>,
  })
  const elsewhere = createRoute({
    getParentRoute: () => root,
    path: '/elsewhere',
    component: () => <p>elsewhere</p>,
  })
  const router = createRouter({
    routeTree: root.addChildren([detail, list, elsewhere]),
    history: createMemoryHistory({ initialEntries, initialIndex }),
  })
  const container = document.createElement('div')
  container.className = 'bg-mixed-background text-on-surface'
  document.body.append(container)
  const reactRoot = createRoot(container)
  reactRoot.render(<RouterProvider router={router} />)
  onTestFinished(() => {
    reactRoot.unmount()
    container.remove()
    queries.clear()
    vi.unstubAllGlobals()
    vi.mocked(ask).mockReset()
  })
  return { router, calls, container }
}

test('a profile that no longer exists shows a way back to the list', async ({
  onTestFinished,
}) => {
  const { router } = await mountDetail({
    items: [],
    initialEntries: ['/main/profiles/profile/detail/missing'],
    onTestFinished,
  })

  await expect
    .element(page.getByText(m.profile_detail_not_found()))
    .toBeVisible()
  await page.getByRole('link', { name: /Profiles/ }).click()
  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/profiles/profile')
})

test('the header is separate from the overview card and goes back through history', async ({
  onTestFinished,
}) => {
  const { router, container } = await mountDetail({
    items: [remote],
    initialEntries: ['/elsewhere', '/main/profiles/profile/detail/remote'],
    initialIndex: 1,
    onTestFinished,
  })

  await expect
    .poll(() =>
      container.querySelector('[data-slot="profile-detail-overview"]'),
    )
    .not.toBeNull()
  const header = container.querySelector('[data-slot="profile-detail-header"]')!
  const card = container.querySelector('[data-slot="profile-detail-overview"]')!
  expect(header.contains(card)).toBe(false)
  expect(card.contains(header)).toBe(false)
  expect(header.textContent).toContain(remote.name)
  expect(card.textContent).not.toContain(remote.name)
  expect(getComputedStyle(header).position).toBe('sticky')

  await page
    .getByRole('button', {
      name: m.profile_detail_back({ destination: m.profile_profile_label() }),
    })
    .click()
  await expect.poll(() => router.state.location.pathname).toBe('/elsewhere')
})

test('the back button falls back to the list without history', async ({
  onTestFinished,
}) => {
  const { router, container } = await mountDetail({
    items: [remote],
    initialEntries: ['/main/profiles/profile/detail/remote'],
    onTestFinished,
  })

  await expect
    .poll(() => container.querySelector('[data-slot="profile-detail-header"]'))
    .not.toBeNull()
  await page
    .getByRole('button', {
      name: m.profile_detail_back({ destination: m.profile_profile_label() }),
    })
    .click()
  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/profiles/profile')
})

test('deleting asks with the profile name and leaves for the list', async ({
  onTestFinished,
}) => {
  const { router, calls } = await mountDetail({
    items: [remote],
    initialEntries: ['/main/profiles/profile/detail/remote'],
    rpc: (method) =>
      method === 'delete_profile'
        ? { status: 'committed', value: null }
        : undefined,
    onTestFinished,
  })
  vi.mocked(ask).mockResolvedValueOnce(false).mockResolvedValueOnce(true)

  const row = page.getByRole('button', {
    name: new RegExp(m.profile_delete_title()),
  })
  await row.click()
  await expect.poll(() => vi.mocked(ask).mock.calls.length).toBe(1)
  expect(vi.mocked(ask).mock.calls[0][0]).toBe(
    m.profile_detail_delete_confirm({ name: remote.name }),
  )
  expect(calls).not.toContain('delete_profile')

  await row.click()
  await expect.poll(() => calls).toContain('delete_profile')
  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/profiles/profile')
})

test('the full sync history unfolds inline instead of opening a drawer', async ({
  onTestFinished,
}) => {
  const runs = [4, 3, 2, 1].map((sequence): RunDto => ({
    id: `run-${sequence}`,
    job: 'profiles/sync/remote',
    definition_version: '1',
    admission_sequence: String(sequence),
    trigger: 'scheduled',
    scheduled_at: null,
    admitted_at: '2026-09-30T00:00:00Z',
    finished_at: '2026-09-30T00:00:01Z',
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
  }))
  const { container } = await mountDetail({
    items: [remote],
    initialEntries: ['/main/profiles/profile/detail/remote?panel=sync'],
    runs,
    onTestFinished,
  })
  const triggers = () =>
    container.querySelectorAll('[data-slot="accordion-trigger"]')

  await expect.poll(() => triggers().length).toBe(3)
  await page
    .getByRole('button', { name: m.profile_detail_history_view_all() })
    .click()
  await expect.poll(() => triggers().length).toBe(4)
  expect(document.querySelector('[role="dialog"]')).toBeNull()

  await page
    .getByRole('button', { name: m.profile_detail_history_hide() })
    .click()
  await expect.poll(() => triggers().length).toBe(3)
})

test('refreshing the subscription does not block saving its options', async ({
  onTestFinished,
}) => {
  const refresh = deferred()
  const sent: string[] = []
  await mountDetail({
    items: [remote],
    initialEntries: ['/main/profiles/profile/detail/remote'],
    rpc: (method) => {
      if (method === 'update_profile') {
        sent.push(method)
        return refresh.promise
      }
      if (method === 'patch_remote_profile_options') {
        sent.push(method)
        return { status: 'committed', value: null }
      }
    },
    onTestFinished,
  })

  await page
    .getByRole('button', { name: m.profile_subscription_update() })
    .click()
  await expect.poll(() => sent).toEqual(['update_profile'])

  await page
    .getByRole('switch', { name: m.profile_update_option_auto_update_label() })
    .click()
  await expect
    .poll(() => sent)
    .toEqual(['update_profile', 'patch_remote_profile_options'])
  refresh.resolve({ status: 'committed', value: null })
})

test('a transform lists where it runs and offers no preview or activation', async ({
  onTestFinished,
}) => {
  const { container } = await mountDetail({
    items: [host, transform],
    globalTransforms: ['fix'],
    initialEntries: ['/main/profiles/javascript/detail/fix'],
    onTestFinished,
  })

  await expect
    .poll(() => container.querySelector('[data-slot="profile-detail-used-by"]'))
    .not.toBeNull()
  const usedBy = container.querySelector(
    '[data-slot="profile-detail-used-by"]',
  )!
  expect(usedBy.textContent).toContain(m.profile_detail_used_by_description())
  const links = Array.from(usedBy.querySelectorAll('a'))
  expect(links.map((link) => link.textContent)).toEqual([
    expect.stringContaining(host.name),
    expect.stringContaining(m.profile_global_transform_title()),
  ])
  expect(links.map((link) => link.getAttribute('href'))).toEqual([
    '/main/profiles/profile/detail/host',
    '/main/profiles/global',
  ])

  const card = container.querySelector('[data-slot="profile-detail-overview"]')!
  expect(card.textContent).not.toContain(transform.name)
  expect(
    container.querySelector('[data-slot="profile-detail-header"]')?.textContent,
  ).toContain(transform.name)
  expect(container.textContent).not.toContain(m.profile_active_title())
  expect(container.textContent).not.toContain(m.profile_chain_editor_title())
  expect(container.querySelector('.monaco-editor, pre, textarea')).toBeNull()
})

test('a transform that no profile uses says so', async ({ onTestFinished }) => {
  const { container } = await mountDetail({
    items: [transform],
    initialEntries: ['/main/profiles/javascript/detail/fix'],
    onTestFinished,
  })

  await expect
    .poll(
      () =>
        container.querySelector('[data-slot="profile-detail-used-by-empty"]')
          ?.textContent,
    )
    .toBe(m.profile_detail_used_by_empty())
})

test('a config lists its chain and the library below it, with chain members pressed', async ({
  onTestFinished,
}) => {
  const { container } = await mountDetail({
    items: [host, transform, other],
    initialEntries: ['/main/profiles/profile/detail/host?panel=transforms'],
    onTestFinished,
  })

  await expect
    .poll(() =>
      Array.from(
        container.querySelectorAll('[data-slot="profile-chain-editor-item"]'),
      ).map((item) => item.getAttribute('data-uid')),
    )
    .toEqual(['fix'])

  const library = container.querySelector(
    '[data-slot="profile-transform-library"]',
  )!
  expect(
    container
      .querySelector('[data-slot="profile-chain-editor-items"]')!
      .compareDocumentPosition(library) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy()
  await expect
    .element(
      page.getByRole('button', {
        name: m.profile_transform_library_toggle({ name: transform.name }),
        exact: true,
      }),
    )
    .toHaveAttribute('aria-pressed', 'true')
  await expect
    .element(
      page.getByRole('button', {
        name: m.profile_transform_library_toggle({ name: other.name }),
        exact: true,
      }),
    )
    .toHaveAttribute('aria-pressed', 'false')
})

const panelEntry = (container: HTMLElement, panel: string) =>
  container.querySelector<HTMLElement>(`[data-panel="${panel}"]`)

const region = (name: string) => page.getByRole('region', { name })

test('a config shows rows for its chain and sync history instead of the editors', async ({
  onTestFinished,
}) => {
  const nextRunAt = '2026-10-01T00:00:00Z'
  const { container } = await mountDetail({
    items: [chained, transform, other],
    runs: [succeeded(1)],
    nextRunAt,
    initialEntries: ['/main/profiles/profile/detail/remote'],
    onTestFinished,
  })

  await expect
    .poll(() => panelEntry(container, 'sync')?.textContent)
    .toContain(m.profile_sync_succeeded())
  expect(panelEntry(container, 'sync')!.textContent).toContain(
    m.profile_sync_next_run({ time: formatDate(nextRunAt) }),
  )
  expect(panelEntry(container, 'sync')!.textContent).toContain(
    m.profile_sync_history_title(),
  )

  const transforms = panelEntry(container, 'transforms')!
  expect(transforms.textContent).toContain(m.profile_chain_editor_title())
  expect(transforms.textContent).toContain(
    m.profile_chain_editor_enabled_count({ count: 1 }),
  )
  expect(transforms.textContent).toContain(transform.name)
  expect(transforms.textContent).not.toContain(other.name)

  expect(transforms.hasAttribute('data-selected')).toBe(false)
  expect(
    container.querySelector('[data-slot="profile-chain-editor"]'),
  ).toBeNull()
  expect(
    container.querySelector('[data-slot="profile-sync-history"]'),
  ).toBeNull()
  expect(
    container.querySelector('[data-slot="profile-detail-panel"]'),
  ).toBeNull()
})

test('a chain without transforms and a profile that never synced say so', async ({
  onTestFinished,
}) => {
  const { container } = await mountDetail({
    items: [remote, transform],
    initialEntries: ['/main/profiles/profile/detail/remote'],
    onTestFinished,
  })

  await expect
    .poll(() => panelEntry(container, 'transforms')?.textContent)
    .toContain(m.profile_chain_editor_empty())
  await expect
    .poll(() => panelEntry(container, 'sync')?.textContent)
    .toContain(m.profile_sync_empty())
})

test('only the rows that apply to a profile are listed', async ({
  onTestFinished,
}) => {
  const { container } = await mountDetail({
    items: [host, transform],
    initialEntries: ['/main/profiles/profile/detail/host'],
    onTestFinished,
  })

  await expect.poll(() => panelEntry(container, 'transforms')).not.toBeNull()
  expect(panelEntry(container, 'sync')).toBeNull()
})

test('rows open and switch the panel through the address, and close it again', async ({
  onTestFinished,
}) => {
  const { router, container } = await mountDetail({
    items: [chained, transform],
    runs: [succeeded(1)],
    initialEntries: ['/main/profiles/profile/detail/remote'],
    onTestFinished,
  })
  const search = () => router.state.location.search as { panel?: string }

  await expect.poll(() => panelEntry(container, 'transforms')).not.toBeNull()
  await panelEntry(container, 'transforms')!.click()
  await expect.poll(() => search().panel).toBe('transforms')
  await expect.element(region(m.profile_chain_editor_title())).toBeVisible()
  // The panel slides in: its slot grows to the width of the panel.
  const slot = () =>
    container.querySelector<HTMLElement>(
      '[data-slot="profile-detail-panel-slot"]',
    )!
  await expect.poll(() => slot().style.width).toBe('auto')
  expect(slot().getBoundingClientRect().width).toBeCloseTo(416, 0)
  // The panel takes the focus and its row is the selected one, beside the
  // detail column.
  await expect
    .poll(() => document.activeElement?.getAttribute('data-slot'))
    .toBe('profile-detail-panel')
  expect(
    panelEntry(container, 'transforms')!.hasAttribute('data-selected'),
  ).toBe(true)
  expect(
    panelEntry(container, 'transforms')!.getAttribute('aria-current'),
  ).toBe('true')
  expect(panelEntry(container, 'sync')!.hasAttribute('data-selected')).toBe(
    false,
  )
  expect(container.querySelector('[data-slot="profile-detail"]')).not.toBeNull()
  expect(
    container
      .querySelector<HTMLElement>('[data-slot="profile-detail-panel"]')!
      .getBoundingClientRect().width,
  ).toBeCloseTo(416, 0)
  // Switching happens through the rows; the panel has no tabs of its own.
  expect(
    container.querySelector('[data-slot="profile-detail-panel"] [role="tab"]'),
  ).toBeNull()
  await expect
    .poll(
      () =>
        container.querySelectorAll('[data-slot="profile-chain-editor-item"]')
          .length,
    )
    .toBe(1)

  await panelEntry(container, 'sync')!.click()
  await expect.poll(() => search().panel).toBe('sync')
  await expect.element(region(m.profile_sync_history_title())).toBeVisible()
  expect(
    container.querySelector('[data-slot="profile-chain-editor"]'),
  ).toBeNull()
  await expect
    .poll(
      () => container.querySelectorAll('[data-slot="profile-sync-run"]').length,
    )
    .toBe(1)

  await page.getByRole('button', { name: m.common_close() }).click()
  await expect.poll(() => search().panel).toBeUndefined()
  // The panel leaves once it has slid out.
  await expect
    .poll(() => container.querySelector('[data-slot="profile-detail-panel"]'))
    .toBeNull()
  // The row of the panel that closed gets the focus back.
  await expect
    .poll(() => document.activeElement?.getAttribute('data-panel'))
    .toBe('sync')

  // History keeps the panel: back reopens it, forward closes it again.
  router.history.back()
  await expect.poll(() => search().panel).toBe('sync')
  await expect.element(region(m.profile_sync_history_title())).toBeVisible()
  router.history.back()
  await expect.poll(() => search().panel).toBe('transforms')
  router.history.forward()
  await expect.poll(() => search().panel).toBe('sync')
})

test('an address with a panel opens it on load', async ({ onTestFinished }) => {
  const { container } = await mountDetail({
    items: [chained, transform],
    initialEntries: ['/main/profiles/profile/detail/remote?panel=transforms'],
    onTestFinished,
  })
  await expect.element(region(m.profile_chain_editor_title())).toBeVisible()
  expect(
    container.querySelector('[data-slot="profile-detail-panel"]'),
  ).not.toBeNull()
})

test.for([
  ['/main/profiles/profile/detail/remote?panel=bogus', [chained]],
  ['/main/profiles/profile/detail/host?panel=sync', [host, transform]],
  ['/main/profiles/javascript/detail/fix?panel=transforms', [transform]],
] as const)('%s opens no panel', async ([entry, items], { onTestFinished }) => {
  const { container } = await mountDetail({
    items: [...items],
    initialEntries: [entry],
    onTestFinished,
  })

  await expect
    .poll(() => container.querySelector('[data-slot="profile-detail-header"]'))
    .not.toBeNull()
  expect(
    container.querySelector('[data-slot="profile-detail-panel"]'),
  ).toBeNull()
})

test('a narrow content area gives the page to the panel, with a back button', async ({
  onTestFinished,
}) => {
  const { router, container } = await mountDetail({
    items: [chained, transform],
    width: 700,
    initialEntries: ['/main/profiles/profile/detail/remote'],
    onTestFinished,
  })
  const search = () => router.state.location.search as { panel?: string }

  await expect.poll(() => panelEntry(container, 'transforms')).not.toBeNull()
  await panelEntry(container, 'transforms')!.click()
  await expect.poll(() => search().panel).toBe('transforms')

  const panel = () =>
    container.querySelector<HTMLElement>('[data-slot="profile-detail-panel"]')
  await expect.poll(() => panel()?.getAttribute('data-layout')).toBe('narrow')
  // The panel covers the detail column and names the profile it belongs to.
  const column = () =>
    container.querySelector<HTMLElement>(
      '[data-slot="profile-detail-scroll-area"]',
    )
  expect(column()!.inert).toBe(true)
  expect(
    panel()!.querySelector('[data-slot="profile-detail-panel-subtitle"]')
      ?.textContent,
  ).toBe(chained.name)
  expect(panel()!.querySelector('h1')?.textContent).toBe(
    m.profile_chain_editor_title(),
  )
  expect(
    panel()!.querySelector('[data-slot="profile-detail-panel-close"]'),
  ).toBeNull()
  expect(
    container.querySelector('[data-slot="profile-transform-library"]'),
  ).toBeNull()
  expect(panel()!.getBoundingClientRect().width).toBeGreaterThan(600)

  await page.getByRole('button', { name: m.profile_back() }).click()
  await expect.poll(() => search().panel).toBeUndefined()
  expect(router.state.location.pathname).toBe(
    '/main/profiles/profile/detail/remote',
  )
  expect(column()!.inert).toBe(false)
  await expect.poll(panel).toBeNull()
  await expect
    .poll(() => document.activeElement?.getAttribute('data-panel'))
    .toBe('transforms')
})

test('closing or switching the panel with unapplied changes asks first', async ({
  onTestFinished,
}) => {
  const { router, container } = await mountDetail({
    items: [remote, transform, other],
    initialEntries: ['/main/profiles/profile/detail/remote?panel=transforms'],
    onTestFinished,
  })
  const search = () => router.state.location.search as { panel?: string }
  const toggle = (name: string) =>
    page.getByRole('button', {
      name: m.profile_transform_library_toggle({ name }),
      exact: true,
    })
  const dialogTitle = () => page.getByText(m.profile_leave_title())
  const apply = () =>
    page.getByRole('button', { name: m.common_apply(), exact: true })

  await toggle(transform.name).click()
  await expect.element(apply()).toBeVisible()

  // Closing and switching both stop at the question. (The memory history of
  // the test cannot block a history pop, so going back is not exercised.)
  await page.getByRole('button', { name: m.common_close() }).click()
  await expect.element(dialogTitle()).toBeVisible()
  expect(search().panel).toBe('transforms')
  await page.getByRole('button', { name: m.profile_leave_stay() }).click()
  await expect.element(dialogTitle()).not.toBeInTheDocument()
  expect(search().panel).toBe('transforms')
  await expect.element(apply()).toBeVisible()
  expect(
    container.querySelectorAll('[data-slot="profile-chain-editor-item"]'),
  ).toHaveLength(1)

  await panelEntry(container, 'sync')!.click()
  await expect.element(dialogTitle()).toBeVisible()
  expect(search().panel).toBe('transforms')
  await page.getByRole('button', { name: m.profile_leave_stay() }).click()
  await expect.element(dialogTitle()).not.toBeInTheDocument()

  // Leaving discards the draft, and the panel opens fresh.
  await page.getByRole('button', { name: m.common_close() }).click()
  await page.getByRole('button', { name: m.profile_leave_confirm() }).click()
  await expect.poll(() => search().panel).toBeUndefined()
  // The panel leaves once it has slid out.
  await expect
    .poll(() => container.querySelector('[data-slot="profile-detail-panel"]'))
    .toBeNull()
  await panelEntry(container, 'transforms')!.click()
  await expect.poll(() => search().panel).toBe('transforms')
  await expect.element(region(m.profile_chain_editor_title())).toBeVisible()
  expect(
    container.querySelectorAll('[data-slot="profile-chain-editor-item"]'),
  ).toHaveLength(0)
  await expect.element(apply()).not.toBeInTheDocument()
})

test('a reset draft leaves the panel without a question', async ({
  onTestFinished,
}) => {
  const { router } = await mountDetail({
    items: [remote, transform],
    initialEntries: ['/main/profiles/profile/detail/remote?panel=transforms'],
    onTestFinished,
  })

  await page
    .getByRole('button', {
      name: m.profile_transform_library_toggle({ name: transform.name }),
      exact: true,
    })
    .click()
  await page
    .getByRole('button', { name: m.common_reset(), exact: true })
    .click()
  await page.getByRole('button', { name: m.common_close() }).click()
  await expect
    .poll(() => (router.state.location.search as { panel?: string }).panel)
    .toBeUndefined()
  expect(document.body.textContent).not.toContain(m.profile_leave_title())
})
