import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as MainIndexRoute } from '../src/pages/(main)/main/index'

function createTree(initialEntry: string) {
  const root = createRootRoute({ component: Outlet })
  const index = createRoute({
    getParentRoute: () => root,
    path: '/main/',
    beforeLoad: (options) =>
      MainIndexRoute.options.beforeLoad?.(options as never),
  })
  const dashboard = createRoute({
    getParentRoute: () => root,
    path: '/main/dashboard',
    component: () => <p>dashboard</p>,
  })
  const router = createRouter({
    routeTree: root.addChildren([index, dashboard]),
    history: createMemoryHistory({ initialEntries: [initialEntry] }),
  })

  return router
}

test.each(['/main', '/main/'])(
  '%s redirects to the dashboard without adding a history entry',
  async (entry) => {
    const router = createTree(entry)

    const view = await render(<RouterProvider router={router} />)

    await expect
      .poll(() => router.state.location.pathname)
      .toBe('/main/dashboard')
    expect(router.history.length).toBe(1)
    expect(router.history.location.state.__TSR_index).toBe(0)

    await view.unmount()
  },
)
