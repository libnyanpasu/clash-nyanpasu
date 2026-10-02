import { useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { DefaultNavbar } from '../src/pages/(main)/_modules/navbar'
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
