import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page } from 'vitest/browser'
import { SharedTooltip } from '@nyanpasu/ui/shared-tooltip'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import { Route } from '@/pages/(main)/main/profiles/$type/detail/$uid'
import { m } from '@/paraglide/messages'
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
  rpc,
  onTestFinished,
}: {
  items: ProfileItem_Serialize[]
  globalTransforms?: string[]
  runs?: RunDto[]
  initialEntries: string[]
  initialIndex?: number
  rpc?: (method: string, params: Record<string, unknown>) => unknown
  onTestFinished: (fn: () => void) => void
}) => {
  await page.viewport(1000, 900)
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
        next_run_at: null,
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
                <ProfilesLayoutProvider>
                  <RouterOutlet />
                </ProfilesLayoutProvider>
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
    initialEntries: ['/main/profiles/profile/detail/remote'],
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
