import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import type { ClashRule } from '@nyanpasu/rpc/types'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as RulesIndexRoute } from '../src/pages/(main)/main/rules/index'
import { Route as RulesRoute } from '../src/pages/(main)/main/rules/route'

const backend = vi.hoisted(() => ({
  rules: [] as ClashRule[],
}))

vi.mock('@nyanpasu/query', () => ({
  useClashRules: () => ({ data: { rules: backend.rules } }),
  useClashProxies: () => ({ proxies: { data: { groups: [] } } }),
  useCurrentProfileUid: () => 'profile',
  useCachedIcon: () => ({ data: undefined }),
  useTrayIcon: () => ({ data: undefined }),
  useWidgetIcon: () => ({ data: undefined }),
}))

vi.mock('../src/pages/(main)/main/rules/_modules/use-rule-stats', () => ({
  useRuleStats: (rules: ClashRule[]) => ({
    live: new Map(
      rules.map((rule, index) => [
        `${rule.type},${rule.payload}`,
        {
          connections: index + 1,
          downloadSpeed: 0,
          uploadSpeed: 0,
        },
      ]),
    ),
    totals: undefined,
  }),
}))

const rootRoute = createRootRoute({
  component: () => (
    <div className="flex h-dvh w-dvw flex-col">
      <Outlet />
    </div>
  ),
})

const rulesRoute = RulesRoute.update({
  id: '/main/rules',
  path: '/main/rules',
  getParentRoute: () => rootRoute,
} as never)

const rulesIndexRoute = RulesIndexRoute.update({
  id: '/main/rules/',
  path: '/',
  getParentRoute: () => rulesRoute,
} as never)

const connectionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/main/connections',
  component: () => <div data-testid="connections-target" />,
})

function makeRules() {
  return Array.from({ length: 120 }, (_, index) => ({
    type: 'DomainSuffix',
    payload: `rule-${index}.example`,
    proxy: 'Proxy',
  })) as ClashRule[]
}

async function openRules(width: number, distinctGroups = false) {
  backend.rules = makeRules().map((rule, index) =>
    distinctGroups ? { ...rule, proxy: `Group ${index}` } : rule,
  )
  await page.viewport(width, 780)

  const router = createRouter({
    routeTree: rootRoute.addChildren([
      rulesRoute.addChildren([rulesIndexRoute]),
      connectionsRoute,
    ]),
    history: createMemoryHistory({ initialEntries: ['/main/rules'] }),
  })

  const view = await render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  )

  await expect.element(view.getByText('rule-0.example')).toBeVisible()

  return { view, router }
}

for (const width of [1440, 420]) {
  test(`rules surface scrolls, sorts and jumps at ${width}px`, async ({
    onTestFinished,
  }) => {
    const { view, router } = await openRules(width)
    onTestFinished(async () => {
      await view.unmount()
      await page.viewport(1280, 800)
    })

    const surface = view.container.querySelector<HTMLElement>(
      '[data-slot="data-page-surface"]',
    )!
    const dataPage = view.container.querySelector<HTMLElement>(
      '[data-slot="data-page"]',
    )!
    const dock = view.container.querySelector<HTMLElement>(
      '[data-slot="data-page-dock"]',
    )!
    const toolbar = view.container.querySelector<HTMLElement>(
      '[data-slot="rules-search"]',
    )!
    expect(dataPage.getAttribute('aria-label')).toBe(m.navbar_label_rules())
    expect(
      view.container.querySelector('[data-slot="data-page-header"]'),
    ).toBeNull()
    expect(dock.contains(toolbar)).toBe(true)

    const surfaceBox = surface.getBoundingClientRect()
    const dockBox = dock.getBoundingClientRect()
    expect(surfaceBox.top).toBeLessThan(dockBox.top)
    expect(dockBox.top).toBeLessThan(surfaceBox.bottom)
    expect(dockBox.bottom).toBeLessThanOrEqual(
      dataPage.getBoundingClientRect().bottom,
    )

    const viewports = surface.querySelectorAll<HTMLElement>(
      '[data-slot="scroll-area-viewport"]',
    )
    expect(viewports).toHaveLength(2)

    const horizontalViewport = viewports[0]
    const verticalViewport = viewports[1]
    expect(surface.clientHeight).toBeGreaterThan(0)
    expect(verticalViewport.clientHeight).toBeGreaterThan(0)
    expect(verticalViewport.clientHeight).toBeLessThan(surface.clientHeight)
    expect(verticalViewport.scrollHeight).toBeGreaterThan(
      verticalViewport.clientHeight,
    )
    if (width < 500) {
      expect(horizontalViewport.scrollWidth).toBeGreaterThan(
        horizontalViewport.clientWidth,
      )
      expect(horizontalViewport.scrollWidth).toBeGreaterThanOrEqual(560)
      horizontalViewport.scrollLeft = 40
      horizontalViewport.dispatchEvent(new Event('scroll'))
      await expect
        .poll(() =>
          surface.querySelector(
            '[data-slot="scroll-area-scrollbar"][data-orientation="horizontal"]',
          ),
        )
        .not.toBeNull()
      const scrollbar = surface.querySelector<HTMLElement>(
        '[data-slot="scroll-area-scrollbar"][data-orientation="horizontal"]',
      )!
      await expect
        .poll(() => scrollbar.getBoundingClientRect().bottom)
        .toBeLessThanOrEqual(dock.getBoundingClientRect().top - 8)
      await expect
        .poll(() => scrollbar.querySelector('[data-slot="scroll-area-thumb"]'))
        .not.toBeNull()
    } else {
      expect(horizontalViewport.scrollWidth).toBeLessThanOrEqual(
        horizontalViewport.clientWidth,
      )
    }

    verticalViewport.scrollTop = verticalViewport.scrollHeight
    verticalViewport.dispatchEvent(new Event('scroll'))
    await expect.element(view.getByText('rule-119.example')).toBeVisible()
    expect(
      view.getByText('rule-119.example').element().getBoundingClientRect()
        .bottom,
    ).toBeLessThanOrEqual(dockBox.top)

    verticalViewport.scrollTop = 0
    verticalViewport.dispatchEvent(new Event('scroll'))
    await view
      .getByRole('button', { name: m.rules_column_connections() })
      .click()
    await expect.element(view.getByText('rule-119.example')).toBeVisible()

    const connections = view.getByRole('button', {
      name: `120 ${m.rules_view_connections()}`,
    })
    await connections.click()
    await expect
      .poll(() => router.state.location.pathname)
      .toBe('/main/connections')
    expect(router.state.location.search).toEqual({
      scope: 'active',
      filters: [{ d: 'rule', v: 'DomainSuffix,rule-119.example' }],
    })
  })
}

test('sidebar last item scrolls above the floating toggle in both widths', async ({
  onTestFinished,
}) => {
  const { view } = await openRules(1280, true)
  onTestFinished(async () => {
    await view.unmount()
    await page.viewport(1280, 800)
  })
  const sidebar = view.container.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar"]',
  )!
  const viewport = sidebar.querySelector<HTMLElement>(
    '[data-slot="scroll-area-viewport"]',
  )!
  const toggle = sidebar.querySelector<HTMLButtonElement>(
    '[data-slot="navigation-sidebar-toggle"]',
  )!
  const lastItem = [
    ...sidebar.querySelectorAll<HTMLElement>(
      '[data-slot="navigation-sidebar-item"]',
    ),
  ].at(-1)!
  for (const width of [88, 200]) {
    await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(width)
    viewport.scrollTop = viewport.scrollHeight
    viewport.dispatchEvent(new Event('scroll'))
    await expect
      .poll(() => lastItem.getBoundingClientRect().bottom)
      .toBeLessThanOrEqual(toggle.getBoundingClientRect().top - 8)
    expect(
      lastItem.querySelector('[data-slot="navigation-sidebar-label"]')
        ?.textContent,
    ).toBe('Group 119')
    toggle.click()
  }
})

test('selecting a sidebar item keeps the sidebar expanded on a narrow viewport', async ({
  onTestFinished,
}) => {
  const { view, router } = await openRules(420)
  onTestFinished(async () => {
    await view.unmount()
    await page.viewport(1280, 800)
  })
  const sidebar = view.container.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar"]',
  )!
  const toggle = sidebar.querySelector<HTMLButtonElement>(
    '[data-slot="navigation-sidebar-toggle"]',
  )!
  toggle.click()
  await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(200)
  await view.getByRole('link', { name: 'Proxy', exact: true }).click()
  await expect
    .poll(() => router.state.location.search)
    .toEqual({ proxy: 'Proxy' })
  expect(toggle.getAttribute('aria-expanded')).toBe('true')
  expect(sidebar.getBoundingClientRect().width).toBe(200)
})
