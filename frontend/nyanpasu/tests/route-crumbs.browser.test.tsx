import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { PageTitle } from '../src/components/page-title'
import type { RouteCrumb } from '../src/components/router/route-crumbs'
import type { FileRoutesByTo } from '../src/route-tree.gen'

const crumb = (title: string, extra?: Partial<RouteCrumb>): RouteCrumb => ({
  title: () => title,
  ...extra,
})

function pageComponent() {
  return function Page() {
    return (
      <ScrollArea>
        <PageTitle />
      </ScrollArea>
    )
  }
}

function createTree(path: string) {
  const root = createRootRoute({ component: Outlet })
  const settings = createRoute({
    getParentRoute: () => root,
    path: '/settings',
    component: pageComponent(),
    staticData: { crumb: crumb('Settings', { hub: true }) },
  })
  const general = createRoute({
    getParentRoute: () => settings,
    path: 'general',
    component: pageComponent(),
    staticData: { crumb: crumb('General') },
  })
  const proxy = createRoute({
    getParentRoute: () => settings,
    path: 'proxy',
    component: Outlet,
    staticData: { crumb: crumb('Proxy') },
  })
  const guard = createRoute({
    getParentRoute: () => proxy,
    path: 'guard',
    component: pageComponent(),
    staticData: { crumb: crumb('Proxy Guard') },
  })
  const router = createRouter({
    routeTree: root.addChildren([
      settings.addChildren([general, proxy.addChildren([guard])]),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  return router
}

test('a subpage shows the crumb title and a semantic parent link on desktop', async ({
  onTestFinished,
}) => {
  await page.viewport(1280, 800)
  onTestFinished(() => page.viewport(414, 896))

  const router = createTree('/settings/proxy/guard')

  const view = await render(<RouterProvider router={router} />)

  await expect.element(view.getByText('Proxy Guard')).toBeInTheDocument()
  const back = view.getByRole('link', {
    name: m.navigation_return_to({ page: 'Proxy' }),
  })
  await expect.element(back).toBeVisible()
  expect(back.element().getAttribute('href')).toBe('/settings/proxy')

  await view.unmount()
})

test('a page directly under a hub hides Back on desktop', async ({
  onTestFinished,
}) => {
  await page.viewport(1280, 800)
  onTestFinished(() => page.viewport(414, 896))

  const router = createTree('/settings/general')

  const view = await render(<RouterProvider router={router} />)

  await expect.element(view.getByText('General')).toBeInTheDocument()
  expect(
    view
      .getByRole('link', {
        name: m.navigation_return_to({ page: 'Settings' }),
      })
      .query(),
  ).toBeNull()

  await view.unmount()
})

test('the string crumb targets name generated routes', () => {
  const targets = [
    '/main/profiles/$type',
    '/main/settings/system/proxy/guard',
    '/main/settings/system/proxy/network-rules',
    '/main/settings/nyanpasu/interface/presets',
    '/main/settings/nyanpasu/interface/custom-css',
  ] as const satisfies readonly (keyof FileRoutesByTo)[]

  expect(targets.length).toBeGreaterThan(0)
})
