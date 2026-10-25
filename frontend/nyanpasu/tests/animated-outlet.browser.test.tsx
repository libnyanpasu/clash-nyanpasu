import { useIsPresent } from 'motion/react'
import { createContext, use, useSyncExternalStore, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  AnimatedOutletPreset,
  usePageEntered,
} from '@/components/router/animated-outlet'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  useLocation,
} from '@tanstack/react-router'

// The outlet freezes the clash streams of a leaving page; none run here.
vi.mock('@nyanpasu/query', () => {
  const Through = ({ children }: { children: ReactNode }) => children
  return {
    ClashWSFreezeBoundary: Through,
    ClashConnectionDetailsFreezeBoundary: Through,
  }
})

// What a layout hands its pages, such as a polled report: a value that
// changes while a page slides out.
const LayoutValue = createContext(0)

const layoutValue = (() => {
  let value = 0
  const listeners = new Set<() => void>()
  return {
    get: () => value,
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    set: (next: number) => {
      value = next
      listeners.forEach((listener) => listener())
    },
  }
})()

const renders = { a: 0, b: 0 }
const presenceRenders = { a: 0, b: 0 }

function PresenceContent({ name }: { name: 'a' | 'b' }) {
  presenceRenders[name] += 1

  return null
}

// A motion component or a nested AnimatePresence in the page: what it
// renders is rendered again when the presence it reads changes.
function Presence({ name }: { name: 'a' | 'b' }) {
  useIsPresent()

  return <PresenceContent name={name} />
}

// Apart from the page, whose renders are counted.
function Entered({ name }: { name: string }) {
  return <p data-entered={name}>{String(usePageEntered())}</p>
}

function Path({ name }: { name: string }) {
  return <p data-path={name}>{useLocation().pathname}</p>
}

function Page({ name }: { name: 'a' | 'b' }) {
  renders[name] += 1

  return (
    <>
      <p data-page={name}>{use(LayoutValue)}</p>
      <Path name={name} />
      <Entered name={name} />
      <Presence name={name} />
    </>
  )
}

function WrappingLayout() {
  const value = useSyncExternalStore(layoutValue.subscribe, layoutValue.get)

  return (
    <AnimatedOutletPreset
      renderOutlet={(outlet) => (
        <LayoutValue value={value}>{outlet}</LayoutValue>
      )}
    />
  )
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
    path: (name: string) =>
      container.querySelector(`[data-path="${name}"]`)?.textContent,
    entered: (name: string) =>
      container.querySelector(`[data-entered="${name}"]`)?.textContent,
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

test('a page sliding out keeps the wrapper it had', async ({
  onTestFinished,
}) => {
  layoutValue.set(0)
  const { router, page } = mount(WrappingLayout, onTestFinished)
  await expect.poll(() => page('a')?.textContent).toBe('0')
  const before = renders.a

  await router.navigate({ to: '/b' as never })
  await expect.poll(() => page('b')).toBeTruthy()
  layoutValue.set(1)

  await expect.poll(() => page('b')?.textContent).toBe('1')
  expect(page('a')?.textContent).toBe('0')
  expect(renders.a).toBe(before)
  await expect.poll(() => page('a')).toBeNull()
})

test('a page sliding out keeps the location it was shown at', async ({
  onTestFinished,
}) => {
  const { router, page, path } = mount(
    () => <AnimatedOutletPreset />,
    onTestFinished,
  )
  await expect.poll(() => path('a')).toBe('/a')

  await router.navigate({ to: '/b' as never })

  await expect.poll(() => page('b')).toBeTruthy()
  expect(path('a')).toBe('/a')
  await expect.poll(() => page('a')).toBeNull()
})

test('a page sliding out is told once that it leaves', async ({
  onTestFinished,
}) => {
  layoutValue.set(0)
  const { router, page } = mount(WrappingLayout, onTestFinished)
  await expect.poll(() => page('a')).toBeTruthy()

  await router.navigate({ to: '/b' as never })
  await expect.poll(() => page('b')).toBeTruthy()
  const before = presenceRenders.a
  // The layout renders again during the slide.
  layoutValue.set(1)

  await expect.poll(() => page('b')?.textContent).toBe('1')
  expect(presenceRenders.a).toBe(before)
  await expect.poll(() => page('a')).toBeNull()
})

test('a page has entered once it slid in, and at once if it did not slide', async ({
  onTestFinished,
}) => {
  const { router, entered } = mount(
    () => <AnimatedOutletPreset />,
    onTestFinished,
  )
  await expect.poll(() => entered('a')).toBe('true')

  await router.navigate({ to: '/b' as never })

  await expect.poll(() => entered('b')).toBe('false')
  await expect.poll(() => entered('b')).toBe('true')
})
