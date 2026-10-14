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
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
  type NavigateOptions,
} from '@tanstack/react-router'
import '../src/utils/language'
import { ReturnButton } from '../src/components/router/return-button'
import {
  toQuery,
  trafficSearchSchema,
} from '../src/pages/(main)/main/topology/_modules/search'
import { TrafficPageContext } from '../src/pages/(main)/main/topology/_modules/traffic-context'
import TrafficToolbar from '../src/pages/(main)/main/topology/_modules/traffic-toolbar'

// The tests project registers the app's router (see the navbar test), so the
// paths of this tree are not part of the registered route types.
const untyped = (options: object) => options as NavigateOptions

// Icons take their real size here, so every control has its real width.
vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

const VIEWPORT_WIDTHS = [320, 480, 672, 768]

async function openToolbar(ticket: boolean) {
  const search = trafficSearchSchema.parse({
    filters: [{ d: 'process', v: 'Browser with a long name' }],
  })

  const rootRoute = createRootRoute({
    component: () => (
      <div data-testid="frame">
        <TrafficPageContext.Provider
          value={{
            search,
            labelOf: (_, key) => ({ text: key, title: key, mono: false }),
            query: toQuery(search),
            empty: false,
            paused: false,
            onSearchChange: () => {},
            onPausedChange: () => {},
            onViewConnections: () => {},
          }}
        >
          <TrafficToolbar>
            <ReturnButton />
          </TrafficToolbar>
        </TrafficPageContext.Provider>
      </div>
    ),
  })

  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  const view = await render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  )
  onTestFinished(() => view.unmount())

  const pause = view.getByRole('button', { name: m.topology_pause() })
  await expect.element(pause).toBeVisible()

  if (ticket) {
    await router.navigate(
      untyped({
        to: '/',
        state: { returnTo: { page: 'connections', href: '/x', index: 0 } },
      }),
    )
    await expect
      .element(
        view.getByRole('button', {
          name: m.navigation_return_to({ page: m.navbar_label_connections() }),
        }),
      )
      .toBeVisible()
  }

  const frame = view.getByTestId('frame').element() as HTMLElement
  const toolbar = frame.querySelector<HTMLElement>(
    '[data-slot=traffic-toolbar]',
  )!

  // Widths are only meaningful once the bar is drawn in Inter.
  await document.fonts.ready
  expect(getComputedStyle(toolbar).fontFamily).toMatch(/^"Inter Variable"/)
  expect(
    [...document.fonts].some(
      (face) =>
        face.family.replaceAll('"', '') === 'Inter Variable' &&
        face.status === 'loaded',
    ),
  ).toBe(true)

  await expect
    .element(view.getByRole('button', { name: m.traffic_metric_label() }))
    .toHaveAttribute('title', m.traffic_metric_bytes())
  await expect
    .element(view.getByRole('button', { name: m.traffic_range_label() }))
    .toHaveAttribute('title', m.traffic_range_last_hour())

  return {
    resize: (width: number) => (frame.style.width = `${width}px`),
    toolbar,
    scope: toolbar.querySelector<HTMLElement>('[data-slot=traffic-scope]')!,
    controls: toolbar.querySelector<HTMLElement>(
      '[data-slot=traffic-controls]',
    )!,
    filters: toolbar.querySelector<HTMLElement>('[data-slot=traffic-filters]')!,
    pause: pause.element(),
  }
}

for (const locale of locales) {
  for (const ticket of [false, true]) {
    test(`${locale}, ${ticket ? 'with' : 'without'} the way back: controls fit and filters dock below`, async () => {
      const previous = getLocale()
      setLocale(locale, { reload: false })
      onTestFinished(() => setLocale(previous, { reload: false }))

      const { resize, toolbar, scope, controls, filters, pause } =
        await openToolbar(ticket)

      for (const width of VIEWPORT_WIDTHS) {
        resize(width)

        expect(toolbar.scrollWidth, `${width}px toolbar`).toBeLessThanOrEqual(
          toolbar.clientWidth,
        )
        expect(
          pause.getBoundingClientRect().right,
          `${width}px`,
        ).toBeLessThanOrEqual(toolbar.getBoundingClientRect().right)

        const toolbarBounds = toolbar.getBoundingClientRect()
        const scopeBounds = scope.getBoundingClientRect()
        const controlsBounds = controls.getBoundingClientRect()
        const filtersBounds = filters.getBoundingClientRect()

        expect(scopeBounds.left, `${width}px scope`).toBeGreaterThanOrEqual(
          toolbarBounds.left,
        )
        expect(controlsBounds.right, `${width}px controls`).toBeLessThanOrEqual(
          toolbarBounds.right,
        )
        expect(
          filtersBounds.top,
          `${width}px filters below controls`,
        ).toBeGreaterThanOrEqual(
          Math.max(scopeBounds.bottom, controlsBounds.bottom),
        )
        expect(filtersBounds.right, `${width}px filters`).toBeLessThanOrEqual(
          toolbarBounds.right,
        )

        for (const slot of [
          'traffic-scope',
          'traffic-metric-range',
          'traffic-actions',
          'traffic-filters',
        ]) {
          const pill = toolbar.querySelector<HTMLElement>(
            `[data-slot=${slot}]`,
          )!
          expect(
            pill.getBoundingClientRect().height,
            `${width}px ${slot}`,
          ).toBe(44)
        }
      }

      resize(768)
      expect(controls.getBoundingClientRect().top).toBe(
        scope.getBoundingClientRect().top,
      )
    })
  }
}
