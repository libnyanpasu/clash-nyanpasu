import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import GroupHeader from '../src/pages/(main)/main/proxies/group/_modules/group-header'
import { Route as ProxiesRoute } from '../src/pages/(main)/main/proxies/route'

test('going back from a group keeps the proxies search', async ({
  onTestFinished,
}) => {
  await page.viewport(414, 896)
  onTestFinished(() => page.viewport(414, 896))
  const root = createRootRoute({ component: Outlet })
  const proxies = createRoute({
    getParentRoute: () => root,
    path: '/main/proxies',
    component: Outlet,
    validateSearch: ProxiesRoute.options.validateSearch,
    staticData: ProxiesRoute.options.staticData,
  })
  const group = createRoute({
    getParentRoute: () => proxies,
    path: 'group/$name',
    component: GroupHeader,
    staticData: { crumb: { title: () => 'A' } },
  })
  const router = createRouter({
    routeTree: root.addChildren([proxies.addChildren([group])]),
    history: createMemoryHistory({
      initialEntries: ['/main/proxies/group/A?q=x'],
    }),
  })

  const view = await render(<RouterProvider router={router} />)

  await view
    .getByRole('link', {
      name: m.navigation_return_to({ page: m.navbar_label_proxies() }),
    })
    .click()

  await expect.poll(() => router.state.location.pathname).toBe('/main/proxies')
  expect(router.state.location.search).toEqual({ q: 'x' })

  await view.unmount()
})
