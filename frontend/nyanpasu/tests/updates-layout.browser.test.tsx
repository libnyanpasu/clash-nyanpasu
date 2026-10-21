import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale, locales, overwriteGetLocale } from '@/paraglide/runtime'
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
import UpdateStatusCard from '../src/pages/(main)/main/settings/nyanpasu/updates/_modules/update-status-card'
import { Route } from '../src/pages/(main)/main/settings/nyanpasu/updates/index'

const backend = vi.hoisted(() => ({
  phase: 'downloading',
  snapshot: {
    revision: 1,
    phase: 'downloading',
    release: { version: '2.1.0', date: null, body: 'Release notes' },
    downloaded: 32 * 1024 * 1024,
    total: 128 * 1024 * 1024,
    speed: 2 * 1024 * 1024,
    source: 'github',
    error: null,
    last_checked_at: '2026-10-03T12:00:00Z',
    supported: true,
    endpoints: ['https://updates.example.test/stable'],
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

test('Update cards remain legible at desktop and mobile widths', async ({
  onTestFinished,
}) => {
  const view = await render(
    <TooltipProvider>
      <ScrollArea className="h-screen w-full">
        <RouterProvider router={router} />
      </ScrollArea>
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    await page.viewport(414, 896)
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
  let masonry = view.container.querySelector<HTMLElement>(
    '[data-slot="updates-masonry"]',
  )
  const viewport = view.container.querySelector<HTMLElement>(
    '[data-slot=scroll-area-viewport]',
  )
  expect(viewport).not.toBeNull()
  expect(masonry).not.toBeNull()

  await page.viewport(1440, 1000)
  expect(getComputedStyle(masonry!).columnCount).toBe('2')
  expect(masonry!.scrollWidth).toBeLessThanOrEqual(viewport!.clientWidth)
  // Masonry packs each column: a card starts one gap below the card above it.
  const columns = Map.groupBy(
    [...cards].map((card) => card.getBoundingClientRect()),
    (rect) => Math.round(rect.left),
  )
  expect(columns.size).toBe(2)
  for (const rects of columns.values()) {
    rects.sort((a, b) => a.top - b.top)
    rects.slice(1).forEach((rect, index) => {
      expect(rect.top - rects[index].bottom).toBeLessThanOrEqual(17)
    })
  }
  const update = view.container.querySelector<HTMLElement>(
    '[data-slot="about-update-card"]',
  )!
  expect(update.getBoundingClientRect().width).toBeLessThan(
    masonry!.clientWidth / 2,
  )
  expect(
    update.querySelector('[data-slot="about-update-controls"]'),
  ).not.toBeNull()
  const sourceCard = view.container.querySelector<HTMLElement>(
    '[data-slot="about-package-source-card"]',
  )!
  const sourceContentHeight = Array.from(sourceCard.children).reduce(
    (height, child) => height + child.getBoundingClientRect().height,
    0,
  )
  expect(sourceCard.getBoundingClientRect().height).toBeLessThanOrEqual(
    sourceContentHeight + 1,
  )
  await page.viewport(390, 844)
  expect(getComputedStyle(masonry!).columnCount).toBe('1')
  expect(masonry!.scrollWidth).toBeLessThanOrEqual(viewport!.clientWidth)
  const preferences = view.container.querySelector<HTMLElement>(
    '[data-slot="about-update-preferences-card"]',
  )
  expect(preferences).not.toBeNull()
  expect(
    preferences!.querySelector(
      '[data-slot="about-release-channel-preference"]',
    ),
  ).not.toBeNull()
  preferences!.scrollIntoView({ block: 'end' })
  expect(preferences!.getBoundingClientRect().bottom).toBeLessThanOrEqual(
    viewport!.getBoundingClientRect().bottom,
  )

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
  masonry = view.container.querySelector<HTMLElement>(
    '[data-slot="updates-masonry"]',
  )
  expect(masonry).not.toBeNull()
  expect(getComputedStyle(masonry!).columnCount).toBe('1')

  await router.navigate({ to: '/main/settings' })
  await view
    .getByRole('link', { name: m.settings_about_update_ready_header() })
    .click()
  expect(router.state.location.pathname).toBe('/main/settings/nyanpasu/updates')
  expect(router.state.location.search).toEqual({})
  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()
})

test('update actions fit every locale at mobile widths in light and dark themes', async ({
  onTestFinished,
}) => {
  const previousGetLocale = getLocale
  const previousSnapshot = backend.snapshot
  const previousPhase = backend.phase
  const wasDark = document.documentElement.classList.contains('dark')
  await page.viewport(390, 844)
  const view = await render(
    <TooltipProvider>
      <UpdateStatusCard />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    overwriteGetLocale(previousGetLocale)
    backend.snapshot = previousSnapshot
    backend.phase = previousPhase
    document.documentElement.classList.toggle('dark', wasDark)
    await page.viewport(414, 896)
  })

  for (const locale of locales) {
    overwriteGetLocale(() => locale)
    let actionWidth: number | undefined
    for (const phase of [
      'up_to_date',
      'available',
      'downloading',
      'verifying',
      'installing',
      'ready',
      'failed',
    ] as const) {
      backend.phase = phase
      backend.snapshot = {
        ...previousSnapshot,
        phase,
        release: phase === 'up_to_date' ? null : previousSnapshot.release,
      }
      for (const dark of [false, true]) {
        document.documentElement.classList.toggle('dark', dark)
        await view.rerender(
          <TooltipProvider>
            <UpdateStatusCard />
          </TooltipProvider>,
        )
        const card = view.container.querySelector<HTMLElement>(
          '[data-slot="about-update-card"]',
        )!
        const icon = card
          .querySelector<HTMLElement>('[data-slot="about-update-status-icon"]')!
          .getBoundingClientRect()
        const statusContent = card
          .querySelector<HTMLElement>(
            '[data-slot="about-update-status-content"]',
          )!
          .getBoundingClientRect()
        expect(
          Math.abs(
            icon.top +
              icon.height / 2 -
              statusContent.top -
              statusContent.height / 2,
          ),
          `${locale} ${phase}: status content must be centered beside the icon`,
        ).toBeLessThan(1)
        const actions = card.querySelector<HTMLElement>(
          '[data-slot="about-update-actions"]',
        )!
        expect(card.scrollWidth, `${locale} ${phase}`).toBeLessThanOrEqual(
          card.clientWidth,
        )
        expect(actions.scrollWidth, `${locale} ${phase}`).toBeLessThanOrEqual(
          actions.clientWidth,
        )
        const actionText = actions.querySelector<HTMLElement>(
          '[data-slot="about-update-label"]',
        )!
        const button = actions.querySelector('button')!.getBoundingClientRect()
        const label = actionText.getBoundingClientRect()
        actionWidth ??= button.width
        expect(
          Math.abs(button.width - actionWidth),
          `${locale} ${phase}: the primary action must keep its width`,
        ).toBeLessThan(1)
        expect(
          Math.abs(
            button.left + button.width / 2 - label.left - label.width / 2,
          ),
          `${locale} ${phase}: loading must not move the label off center`,
        ).toBeLessThan(1)
        expect(
          actionText.scrollWidth,
          `${locale} ${phase}`,
        ).toBeLessThanOrEqual(actionText.clientWidth)
      }
    }
  }
})
