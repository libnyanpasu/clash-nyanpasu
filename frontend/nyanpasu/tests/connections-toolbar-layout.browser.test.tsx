// Layout is measured with the app's Inter font, theme and Tailwind classes.
// Scripts Inter does not cover (CJK, Hangul) fall back to the platform fonts.
import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale, locales, setLocale } from '@/paraglide/runtime'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
  type NavigateOptions,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import '../src/utils/language'
import { ReturnButton } from '../src/components/router/return-button'
import {
  DataPage,
  DataPageSurface,
} from '../src/pages/(main)/main/_modules/data-page'
import ConnectionsToolbar from '../src/pages/(main)/main/connections/_modules/connections-toolbar'
import { StatusTabs } from '../src/pages/(main)/main/connections/_modules/status-tabs'
import { TestQueryProvider } from './query-provider'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

// The tests project registers the app's router (see the navbar test), so the
// paths of this tree are not part of the registered route types.
const untyped = (options: object) => options as NavigateOptions

// Icons take their real size here, so every control has its real width.
vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

// The dock's side insets leave a bar 40px narrower than its page on desktop.
// At 936px the control group can still wrap its chips; it fits on one line at
// 1152px across the supported locales.
const WRAPPED = [208, 240, 320, 480, 672, 768, 895, 896, 936]
const ONE_LINE = [1152]

async function openToolbar(ticket: boolean) {
  // Profile names are unknown here, so the chips show the raw keys.
  mockIPC(() => {
    throw new Error('unavailable')
  })

  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })

  onTestFinished(() => {
    queries.clear()
    clearMocks()
  })

  // A jump from the traffic page: a filter, a range and large counts.
  const rootRoute = createRootRoute({
    component: () => (
      <div className="flex min-h-0 w-full" data-testid="frame">
        <DataPage
          title={m.navbar_label_connections()}
          toolbar={
            <ConnectionsToolbar
              start={<ReturnButton />}
              tabs={
                <StatusTabs
                  value="active"
                  onValueChange={() => {}}
                  activeCount={1_234}
                  closedCount={123_456}
                />
              }
              selection={{
                range: 'last24_hours',
                filters: [{ d: 'rule', v: 'DomainSuffix,example.com' }],
              }}
              onSelectionChange={() => {}}
              search=""
              onSearchChange={() => {}}
              onOpenSettings={() => {}}
              onCloseAll={() => {}}
            />
          }
        >
          <DataPageSurface />
        </DataPage>
      </div>
    ),
  })

  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  const view = await render(
    <TestQueryProvider client={queries}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </TestQueryProvider>,
  )
  onTestFinished(() => view.unmount())

  await expect
    .element(view.getByRole('group', { name: m.traffic_filters_label() }))
    .toBeVisible()

  if (ticket) {
    await router.navigate(
      untyped({
        to: '/',
        state: { returnTo: { page: 'traffic', href: '/x', index: 0 } },
      }),
    )
    await expect
      .element(
        view.getByRole('button', {
          name: m.navigation_return_to({ page: m.topology_title() }),
        }),
      )
      .toBeVisible()
  }

  const frame = view.getByTestId('frame').element() as HTMLElement
  frame.style.height = '640px'
  const page = frame.querySelector<HTMLElement>('[data-slot=data-page]')!
  const dock = frame.querySelector<HTMLElement>('[data-slot=data-page-dock]')!
  const toolbar = frame.querySelector<HTMLElement>(
    '[data-slot=connections-toolbar]',
  )!
  const tabs = frame.querySelector<HTMLElement>(
    '[data-slot=data-page-controls]',
  )!
  const surface = frame.querySelector('[data-slot=data-page-surface]')!
  const row = toolbar
  const actions = toolbar.querySelector('[data-slot=connections-actions]')!

  expect(page.getAttribute('aria-label')).toBe(m.navbar_label_connections())
  expect(frame.querySelector('[data-slot=data-page-header]')).toBeNull()
  expect(dock.contains(toolbar)).toBe(true)
  expect(toolbar.contains(tabs)).toBe(true)

  // Widths are only meaningful once the bar is drawn in Inter.
  await document.fonts.ready
  expect(getComputedStyle(row).fontFamily).toMatch(/^"Inter Variable"/)

  return {
    resize: (width: number) => (frame.style.width = `${width}px`),
    row,
    page,
    dock,
    surface,
    iconButtons: [...actions.querySelectorAll(':scope > button')],
    search: row.querySelector('[data-slot=connections-search]')!.parentElement!,
    chips: row.querySelector('[data-slot=connections-filters]')!,
    tabs,
  }
}

for (const locale of locales) {
  for (const ticket of [false, true]) {
    test(`${locale}, ${ticket ? 'with' : 'without'} the way back: no control overflows the floating dock`, async () => {
      const previous = getLocale()
      setLocale(locale, { reload: false })
      onTestFinished(() => setLocale(previous, { reload: false }))

      const {
        resize,
        row,
        page,
        dock,
        surface,
        iconButtons,
        search,
        chips,
        tabs,
      } = await openToolbar(ticket)

      for (const width of [...WRAPPED, ...ONE_LINE]) {
        resize(width)

        const dockBox = dock.getBoundingClientRect()
        const surfaceBox = surface.getBoundingClientRect()
        const rowBox = row.getBoundingClientRect()
        expect(row.scrollWidth, `${width}px`).toBeLessThanOrEqual(
          row.clientWidth,
        )
        expect(rowBox.left).toBeGreaterThanOrEqual(dockBox.left)
        expect(rowBox.right).toBeLessThanOrEqual(dockBox.right)
        expect(rowBox.top).toBeLessThan(surfaceBox.bottom)
        expect(surfaceBox.top).toBeLessThan(rowBox.top)
        expect(dockBox.bottom).toBeLessThanOrEqual(
          page.getBoundingClientRect().bottom,
        )

        // A crowded row wraps the search field rather than squeezing it.
        expect(
          search.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThanOrEqual(Math.min(192, row.clientWidth))

        // A crowded row wraps the tab group while keeping the icon buttons intact.
        expect(
          tabs.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThan(0)
        for (const button of iconButtons) {
          const { width: buttonWidth, right } = button.getBoundingClientRect()

          expect(buttonWidth, `${width}px`).toBe(40)
          expect(right, `${width}px`).toBeLessThanOrEqual(
            row.getBoundingClientRect().right,
          )
        }
      }

      // The widest layout keeps the chips and tabs visible inside the dock.
      for (const width of ONE_LINE) {
        resize(width)

        expect(row.getBoundingClientRect().height, `${width}px`).toBeLessThan(
          112,
        )
        expect(
          chips.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThan(0)
        expect(
          tabs.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThan(0)
      }
    })
  }
}
