import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as ProxiesRoute } from '../src/pages/(main)/main/proxies/route'

test('going back from a group keeps the proxies search', async () => {
  const root = createRootRoute({ component: Outlet })
  const proxies = createRoute({
    getParentRoute: () => root,
    path: '/main/proxies',
    component: Outlet,
    validateSearch: ProxiesRoute.options.validateSearch,
    search: ProxiesRoute.options.search,
  })
  const group = createRoute({
    getParentRoute: () => proxies,
    path: 'group/$name',
    component: () => <Link to="/main/proxies">back</Link>,
  })
  const router = createRouter({
    routeTree: root.addChildren([proxies.addChildren([group])]),
    history: createMemoryHistory({
      initialEntries: ['/main/proxies/group/A?q=x'],
    }),
  })

  const view = await render(<RouterProvider router={router} />)

  await view.getByRole('link', { name: 'back' }).click()

  await expect.poll(() => router.state.location.pathname).toBe('/main/proxies')
  expect(router.state.location.search).toEqual({ q: 'x' })

  await view.unmount()
})
