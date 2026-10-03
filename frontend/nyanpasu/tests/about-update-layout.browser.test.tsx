import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route } from '../src/pages/(main)/main/settings/about/route'

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
  },
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
vi.mock('@/components/logo/animated-logo', () => ({
  default: ({ className }: { className?: string }) => (
    <div className={className} data-slot="animated-logo" />
  ),
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
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({ commands: { openThat: vi.fn() } }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

const AboutPage = Route.options.component ?? (() => null)
const rootRoute = createRootRoute({ component: () => <Outlet /> })
const aboutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: AboutPage,
})
const router = createRouter({
  routeTree: rootRoute.addChildren([aboutRoute]),
  history: createMemoryHistory({ initialEntries: ['/'] }),
})

test('About update cards remain legible at desktop and mobile widths', async ({
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
  let grid = view.container.querySelector<HTMLElement>('.grid.grid-cols-1')
  const viewport = view.container.querySelector<HTMLElement>(
    '[data-slot=scroll-area-viewport]',
  )
  expect(viewport).not.toBeNull()
  expect(grid).not.toBeNull()

  await page.viewport(1440, 1000)
  expect(getComputedStyle(grid!).gridTemplateColumns.split(' ')).toHaveLength(2)
  expect(grid!.scrollWidth).toBeLessThanOrEqual(viewport!.clientWidth)
  const update = view.container.querySelector<HTMLElement>(
    '[data-slot="about-version-card"]',
  )!
  expect(update.getBoundingClientRect().width).toBeLessThan(
    grid!.clientWidth / 2,
  )
  expect(
    update.querySelector('[data-slot="about-update-controls"]'),
  ).not.toBeNull()
  const sourceCard = view.container.querySelector<HTMLElement>(
    '[data-slot="about-package-source-card"]',
  )!
  expect(getComputedStyle(sourceCard).gridRowEnd).toBe('auto')
  const sourceContentHeight = Array.from(sourceCard.children).reduce(
    (height, child) => height + child.getBoundingClientRect().height,
    0,
  )
  expect(sourceCard.getBoundingClientRect().height).toBeLessThanOrEqual(
    sourceContentHeight + 1,
  )
  const logo = view.container.querySelector<HTMLElement>(
    '[data-slot="animated-logo"]',
  )!
  expect(logo.getBoundingClientRect().width).toBe(128)
  expect(logo.getBoundingClientRect().height).toBe(128)

  await page.viewport(390, 844)
  expect(getComputedStyle(grid!).gridTemplateColumns.split(' ')).toHaveLength(1)
  expect(grid!.scrollWidth).toBeLessThanOrEqual(viewport!.clientWidth)
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
  grid = view.container.querySelector<HTMLElement>('.grid.grid-cols-1')
  expect(grid).not.toBeNull()
  expect(getComputedStyle(grid!).gridTemplateColumns.split(' ')).toHaveLength(1)
})
