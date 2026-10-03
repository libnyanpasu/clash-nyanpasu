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

// The bar goes on one line from @4xl (896px), the search field widening from
// @6xl (1152px). 895 is the widest wrapped bar.
const WRAPPED = [480, 672, 768, 895]
const ONE_LINE = [896, 1152]

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
      <div data-testid="frame">
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
  const row = frame.querySelector('[data-slot=connections-toolbar] > div')!

  // Widths are only meaningful once the bar is drawn in Inter.
  await document.fonts.ready
  expect(getComputedStyle(row).fontFamily).toMatch(/^"Inter Variable"/)

  return {
    resize: (width: number) => (frame.style.width = `${width}px`),
    row,
    // The settings and close-all buttons, which end the bar.
    iconButtons: [...row.querySelectorAll(':scope > button')].slice(-2),
    search: row.querySelector('[data-slot=connections-search]')!,
    chips: row.querySelector('[data-slot=connections-filters]')!,
  }
}

for (const locale of locales) {
  for (const ticket of [false, true]) {
    test(`${locale}, ${ticket ? 'with' : 'without'} the way back: no control overflows the bar`, async () => {
      const previous = getLocale()
      setLocale(locale, { reload: false })
      onTestFinished(() => setLocale(previous, { reload: false }))

      const { resize, row, iconButtons, search, chips } =
        await openToolbar(ticket)

      for (const width of [...WRAPPED, ...ONE_LINE]) {
        resize(width)

        expect(row.scrollWidth, `${width}px`).toBeLessThanOrEqual(
          row.clientWidth,
        )

        // A crowded row wraps the search field rather than squeezing it.
        expect(
          search.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThanOrEqual(192)

        // A crowded row squeezes the icon buttons before it overflows.
        for (const button of iconButtons) {
          const { width: buttonWidth, right } = button.getBoundingClientRect()

          expect(buttonWidth, `${width}px`).toBe(40)
          expect(right, `${width}px`).toBeLessThanOrEqual(
            row.getBoundingClientRect().right,
          )
        }
      }

      // One line: the bar keeps its single-row height, the chips in it.
      for (const width of ONE_LINE) {
        resize(width)

        expect(row.getBoundingClientRect().height, `${width}px`).toBe(64)
        expect(
          chips.getBoundingClientRect().width,
          `${width}px`,
        ).toBeGreaterThan(0)
      }
    })
  }
}
