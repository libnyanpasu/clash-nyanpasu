import { useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import {
  DefaultNavbar,
  MobileNavbar,
} from '../src/pages/(main)/_modules/navbar'
import { Route as ConnectionsRoute } from '../src/pages/(main)/main/connections/route'
import { Route as ProxiesRoute } from '../src/pages/(main)/main/proxies/route'
import { Route as RulesRoute } from '../src/pages/(main)/main/rules/route'
import type { routeTree } from '../src/route-tree.gen'

// The app registers its router in main.tsx, which this project does not
// compile; the navbar's typed links need the registration.
declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createRouter<typeof routeTree>>
  }
}

// The proxies query, published directly by the test.
const proxies = vi.hoisted(() => {
  let data: unknown = undefined
  const listeners = new Set<() => void>()
  return {
    get: () => data,
    publish(next: unknown) {
      data = next
      listeners.forEach((listener) => listener())
    },
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
  }
})

vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useClashProxies: () => ({
    proxies: { data: useSyncExternalStore(proxies.subscribe, proxies.get) },
  }),
}))

// Every navbar button matches its route once per render.
const matchRouteCalls = vi.hoisted(() => ({ count: 0 }))

vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...actual,
    useMatchRoute: () => {
      matchRouteCalls.count += 1
      return actual.useMatchRoute()
    },
  }
})

const snapshot = (delay: number) => ({
  groups: [{ name: 'Proxy', hidden: false }],
  nodes: { a: { name: 'a', history: [{ time: '', delay }] } },
})

test('proxies updates re-render only the proxies button', async ({
  onTestFinished,
}) => {
  proxies.publish(snapshot(0))

  const router = createRouter({
    routeTree: createRootRoute({ component: DefaultNavbar }),
    history: createMemoryHistory({ initialEntries: ['/main/dashboard'] }),
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<RouterProvider router={router} />)

  await expect
    .poll(() => container.querySelectorAll('[role="tab"]').length)
    .toBe(9)
  const proxiesLink = () =>
    container.querySelector('a[href*="/main/proxies"]')?.getAttribute('href')
  expect(proxiesLink()).toBe('/main/proxies/group/Proxy')

  const before = matchRouteCalls.count
  // A group delay test writes results into the cache repeatedly.
  for (let delay = 1; delay <= 5; delay++) {
    proxies.publish(snapshot(delay))
    await new Promise((resolve) => requestAnimationFrame(resolve))
  }
  expect(matchRouteCalls.count - before).toBe(0)

  proxies.publish({ ...snapshot(0), groups: [{ name: 'Auto', hidden: false }] })
  await expect.poll(proxiesLink).toBe('/main/proxies/group/Auto')
})

const entryCases = [
  {
    source: 'connections',
    Navbar: DefaultNavbar,
    destination: '/main/proxies/group/Proxy',
  },
  {
    source: 'rules',
    Navbar: DefaultNavbar,
    destination: '/main/proxies/group/Proxy',
  },
  { source: 'connections', Navbar: MobileNavbar, destination: '/main/proxies' },
  { source: 'rules', Navbar: MobileNavbar, destination: '/main/proxies' },
] as const

test.each(entryCases)(
  'entering $destination from $source clears the previous search',
  async ({ source, Navbar, destination }) => {
    proxies.publish(snapshot(0))
    const root = createRootRoute({
      component: () => (
        <>
          <Navbar />
          <Outlet />
        </>
      ),
    })
    const proxiesRoute = createRoute({
      getParentRoute: () => root,
      path: '/main/proxies',
      validateSearch: ProxiesRoute.options.validateSearch,
      search: ProxiesRoute.options.search,
      component: Outlet,
    })
    const group = createRoute({
      getParentRoute: () => proxiesRoute,
      path: 'group/$name',
    })
    const connections = createRoute({
      getParentRoute: () => root,
      path: '/main/connections',
      validateSearch: ConnectionsRoute.options.validateSearch,
    })
    const rules = createRoute({
      getParentRoute: () => root,
      path: '/main/rules',
      validateSearch: RulesRoute.options.validateSearch,
    })
    const router = createRouter({
      routeTree: root.addChildren([
        connections,
        rules,
        proxiesRoute.addChildren([group]),
      ]),
      history: createMemoryHistory({
        initialEntries: [`/main/${source}?q=foreign`],
      }),
    })
    const view = await render(<RouterProvider router={router} />)
    await expect
      .poll(() => router.state.location.search)
      .toEqual({ q: 'foreign' })
    const proxyLink = view.container.querySelector<HTMLAnchorElement>(
      'a[href*="/main/proxies"]',
    )
    expect(proxyLink).not.toBeNull()
    proxyLink?.click()
    await expect.poll(() => router.state.location.pathname).toBe(destination)
    expect(router.state.location.search).toEqual({})
    await view.unmount()
  },
)
