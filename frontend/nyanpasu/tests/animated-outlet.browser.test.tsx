import { type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'

// The outlet freezes the clash streams of a leaving page; none run here.
vi.mock('@nyanpasu/query', () => {
  const Through = ({ children }: { children: ReactNode }) => children
  return {
    ClashWSFreezeBoundary: Through,
    ClashConnectionDetailsFreezeBoundary: Through,
  }
})

const renders = { a: 0, b: 0 }

function Page({ name }: { name: 'a' | 'b' }) {
  renders[name] += 1

  return <p data-page={name}>{name}</p>
}

function mount(
  layout: () => ReactNode,
  onTestFinished: (fn: () => void) => void,
) {
  const root = createRootRoute({ component: layout })
  const page = (name: 'a' | 'b') =>
    createRoute({
      path: `/${name}`,
      getParentRoute: () => root,
      component: () => <Page name={name} />,
    })
  const router = createRouter({
    routeTree: root.addChildren([page('a'), page('b')]),
    history: createMemoryHistory({ initialEntries: ['/a'] }),
  })

  const container = document.createElement('div')
  document.body.append(container)
  const reactRoot = createRoot(container)
  onTestFinished(() => {
    reactRoot.unmount()
    container.remove()
  })
  reactRoot.render(<RouterProvider router={router as never} />)

  return {
    router,
    page: (name: string) => container.querySelector(`[data-page="${name}"]`),
  }
}

test('a page sliding out stays the page it was and is not re-rendered', async ({
  onTestFinished,
}) => {
  const { router, page } = mount(() => <AnimatedOutletPreset />, onTestFinished)
  await expect.poll(() => page('a')).toBeTruthy()
  const before = renders.a

  await router.navigate({ to: '/b' as never })

  await expect.poll(() => page('b')).toBeTruthy()
  expect(page('a')).toBeTruthy()
  expect(renders.a).toBe(before)
  await expect.poll(() => page('a')).toBeNull()
})
