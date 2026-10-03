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
import { trafficSearchSchema } from '../src/pages/(main)/main/topology/_modules/search'
import TrafficToolbar from '../src/pages/(main)/main/topology/_modules/traffic-toolbar'

// The tests project registers the app's router (see the navbar test), so the
// paths of this tree are not part of the registered route types.
const untyped = (options: object) => options as NavigateOptions

// Icons take their real size here, so every control has its real width.
vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

// The bar goes on one line from @3xl (768px). 767 is the widest wrapped bar,
// 768 the narrowest single line.
const WRAPPED = [480, 672, 767]
const ONE_LINE = 768

async function openToolbar(ticket: boolean) {
  const rootRoute = createRootRoute({
    component: () => (
      <div data-testid="frame">
        <TrafficToolbar
          search={trafficSearchSchema.parse({})}
          paused={false}
          labelOf={(_, key) => ({ text: key, title: key, mono: false })}
          onSearchChange={() => {}}
          onPausedChange={() => {}}
          onViewConnections={() => {}}
          start={<ReturnButton />}
        />
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
  const row = frame.querySelector('[data-slot=traffic-toolbar] > div')!

  // Widths are only meaningful once the bar is drawn in Inter.
  await document.fonts.ready
  expect(getComputedStyle(row).fontFamily).toMatch(/^"Inter Variable"/)
  expect(
    [...document.fonts].some(
      (face) =>
        face.family.replaceAll('"', '') === 'Inter Variable' &&
        face.status === 'loaded',
    ),
  ).toBe(true)

  return {
    resize: (width: number) => (frame.style.width = `${width}px`),
    row,
    pause: pause.element(),
  }
}

for (const locale of locales) {
  for (const ticket of [false, true]) {
    test(`${locale}, ${ticket ? 'with' : 'without'} the way back: no control overflows the bar`, async () => {
      const previous = getLocale()
      setLocale(locale, { reload: false })
      onTestFinished(() => setLocale(previous, { reload: false }))

      const { resize, row, pause } = await openToolbar(ticket)

      for (const width of [...WRAPPED, ONE_LINE]) {
        resize(width)

        expect(row.scrollWidth, `${width}px`).toBeLessThanOrEqual(
          row.clientWidth,
        )
        expect(
          pause.getBoundingClientRect().right,
          `${width}px`,
        ).toBeLessThanOrEqual(row.getBoundingClientRect().right)
      }

      // One line: the bar keeps its single-row height.
      resize(ONE_LINE)
      expect(row.getBoundingClientRect().height).toBe(64)
    })
  }
}
