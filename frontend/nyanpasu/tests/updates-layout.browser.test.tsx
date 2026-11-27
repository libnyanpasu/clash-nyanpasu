import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import type { AppUpdateSnapshot } from '@nyanpasu/rpc/types'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import HeaderMenu from '../src/pages/(main)/_modules/header-menu'
import { Route } from '../src/pages/(main)/main/settings/nyanpasu/updates/index'
import { AllPermissions } from './all-permissions'

const backend = vi.hoisted(() => ({
  phase: 'downloading',
  snapshot: {
    revision: 1,
    phase: 'downloading',
    release: { version: '2.1.0', date: null, body: null },
    downloaded: 32 * 1024 * 1024,
    total: 128 * 1024 * 1024,
    speed: 2 * 1024 * 1024,
    source: 'github',
    error: null,
    last_checked_at: '2026-10-03T12:00:00Z',
    supported: true,
    endpoints: [],
  } as AppUpdateSnapshot,
  check: vi.fn(),
  download: vi.fn(),
  cancelDownload: vi.fn(),
  install: vi.fn(),
  discardPackage: vi.fn(),
  upsert: vi.fn(),
}))

vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({
    snapshot: { ...backend.snapshot, phase: backend.phase },
    currentVersion: '2.0.0',
    hasNewVersion: Boolean(backend.snapshot.release),
    isReady: backend.phase === 'ready',
    isDesktop: true,
    isBusy: [
      'checking',
      'downloading',
      'cancelling',
      'verifying',
      'installing',
    ].includes(backend.phase),
    isLoading: false,
    isPending: false,
    check: backend.check,
    download: backend.download,
    cancelDownload: backend.cancelDownload,
    install: backend.install,
    discardPackage: backend.discardPackage,
  }),
}))
vi.mock('../src/pages/(main)/_modules/header-file-action', () => ({
  default: () => null,
}))
vi.mock('../src/pages/(main)/_modules/header-settings-action', () => ({
  default: () => null,
}))
vi.mock('../src/pages/(main)/_modules/header-help-action', () => ({
  default: () => null,
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({
    value:
      key === 'update_sources'
        ? ['nyanpasu', 'github']
        : key === 'enable_auto_check_update',
    isPending: false,
    upsert: backend.upsert,
    refetch: vi.fn(),
  }),
  useReleaseChannel: () => ({
    query: { data: { current: 'stable', installed: 'stable' } },
    mutation: { mutateAsync: vi.fn(), isPending: false },
  }),
  useAppUpdateChangelog: () => ({ isPending: true }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({ commands: { openThat: vi.fn() } }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

const UpdatesPage = Route.options.component ?? (() => null)
const rootRoute = createRootRoute({
  component: () => (
    <>
      <HeaderMenu />
      <Outlet />
    </>
  ),
})

const updatesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/main/settings/nyanpasu/updates',
  component: UpdatesPage,
})
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/main/settings',
  component: () => null,
})
const router = createRouter({
  routeTree: rootRoute.addChildren([updatesRoute, settingsRoute]),
  history: createMemoryHistory({
    initialEntries: ['/main/settings/nyanpasu/updates?action=need-update'],
  }),
})

test('Update cards expose expected sections and navigate cleanly without opening an error dialog', async ({
  onTestFinished,
}) => {
  const view = await render(
    <TooltipProvider>
      <ScrollArea className="h-screen w-full">
        <RouterProvider router={router} />
      </ScrollArea>
    </TooltipProvider>,

    { wrapper: AllPermissions },
  )
  onTestFinished(async () => {
    await view.unmount()
  })

  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()

  const cards = view.container.querySelectorAll(
    '[data-slot^="about-"][data-slot$="-card"]',
  )
  expect(cards).toHaveLength(3)
  expect(
    view.container.querySelector('[data-slot="about-release-channel-card"]'),
  ).toBeNull()
  expect(
    view.container.querySelector('[data-slot="about-update-endpoints-card"]'),
  ).toBeNull()
  const masonry = view.container.querySelector<HTMLElement>(
    '[data-slot="updates-masonry"]',
  )
  expect(masonry).not.toBeNull()
  const update = view.container.querySelector<HTMLElement>(
    '[data-slot="about-update-card"]',
  )!
  expect(
    update.querySelector('[data-slot="about-update-controls"]'),
  ).not.toBeNull()
  const preferences = view.container.querySelector<HTMLElement>(
    '[data-slot="about-update-preferences-card"]',
  )
  expect(preferences).not.toBeNull()
  expect(
    preferences!.querySelector(
      '[data-slot="about-release-channel-preference"]',
    ),
  ).not.toBeNull()

  backend.phase = 'ready'
  backend.snapshot = {
    ...backend.snapshot,
    revision: backend.snapshot.revision + 1,
    phase: 'ready',
    downloaded: 128 * 1024 * 1024,
    total: 128 * 1024 * 1024,
  }
  await view.rerender(
    <TooltipProvider>
      <ScrollArea className="h-screen w-full">
        <RouterProvider router={router} key="ready" />
      </ScrollArea>
    </TooltipProvider>,
  )
  expect(
    view.container.querySelector('[data-slot="updates-masonry"]'),
  ).not.toBeNull()

  await router.navigate({ to: '/main/settings' })
  await view
    .getByRole('link', { name: m.settings_about_update_ready_header() })
    .click()
  expect(router.state.location.pathname).toBe('/main/settings/nyanpasu/updates')
  expect(router.state.location.search).toEqual({})
  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()
})
