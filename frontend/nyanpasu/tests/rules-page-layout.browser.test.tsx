// Tailwind's flex/sr-only utilities keep the badge and sr-only label
// space-separated in the accessible name, and h-dvh sizes the scroll viewport.
import '@/assets/styles/tailwind.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
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
  useProxyGroupIcons: () => ({}),
  useCurrentProfileUid: () => 'profile',
  useCachedIcon: () => undefined,
  useTrayIcon: () => undefined,
  useWidgetIcon: () => undefined,
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

async function openRules(distinctGroups = false) {
  backend.rules = makeRules().map((rule, index) =>
    distinctGroups ? { ...rule, proxy: `Group ${index}` } : rule,
  )

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

test('rules surface sorts by connections and jumps to filtered connections', async ({
  onTestFinished,
}) => {
  const { view, router } = await openRules()
  onTestFinished(() => view.unmount())

  await view.getByRole('button', { name: m.rules_column_connections() }).click()
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

test('selecting a sidebar item keeps the sidebar expanded', async ({
  onTestFinished,
}) => {
  const { view, router } = await openRules()
  onTestFinished(() => view.unmount())

  const toggle = view.container.querySelector<HTMLButtonElement>(
    '[data-slot="navigation-sidebar-toggle"]',
  )!
  toggle.click()
  await expect.poll(() => toggle.getAttribute('aria-expanded')).toBe('true')
  await view.getByRole('link', { name: 'Proxy', exact: true }).click()
  await expect
    .poll(() => router.state.location.search)
    .toEqual({ proxy: 'Proxy' })
  expect(toggle.getAttribute('aria-expanded')).toBe('true')
})
