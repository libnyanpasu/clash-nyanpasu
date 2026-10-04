import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import z from 'zod'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type NavigateOptions,
} from '@tanstack/react-router'
import {
  keepReturn,
  type CrossPage,
} from '../src/components/router/cross-navigation'
import { ReturnButton } from '../src/components/router/return-button'
import {
  useCrossNavigate,
  useEntryFocus,
} from '../src/components/router/use-cross-navigate'

// The tests project registers the app's router (see the navbar test), so the
// paths of this tree are not part of the registered route types.
const untyped = (options: object) => options as NavigateOptions

function Origin() {
  const crossNavigate = useCrossNavigate()

  const focus = useEntryFocus()

  return (
    <>
      <ReturnButton />

      <p data-testid="origin-focus">{focus ?? 'none'}</p>

      <button
        type="button"
        onClick={() =>
          crossNavigate({
            from: 'rules' satisfies CrossPage,
            originFocus: 'row-7',
            to: untyped({ to: '/b' }),
          })
        }
      >
        jump
      </button>
    </>
  )
}

function Target() {
  const push = (n: number) =>
    router.navigate(untyped({ to: '/b', search: { n }, state: keepReturn }))

  return (
    <>
      <ReturnButton />

      <button type="button" onClick={() => push(1)}>
        first
      </button>

      <button type="button" onClick={() => push(2)}>
        second
      </button>
    </>
  )
}

const rootRoute = createRootRoute({ component: Outlet })

const routeTree = rootRoute.addChildren([
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/a',
    component: Origin,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/b',
    component: Target,
    validateSearch: z.object({ n: z.number().optional() }),
  }),
])

const router = createRouter({
  routeTree,
  history: createMemoryHistory({ initialEntries: ['/a'] }),
})

const indexOf = () => router.history.location.state.__TSR_index

test('a jump offers the way back to the origin entry and its focus', async () => {
  const screen = await render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  )

  const back = m.navigation_return_to({ page: m.navbar_label_rules() })

  await expect
    .element(screen.getByRole('button', { name: 'jump' }))
    .toBeVisible()
  expect(screen.getByRole('button', { name: back }).query()).toBeNull()
  expect(screen.getByTestId('origin-focus')).toHaveTextContent('none')

  const originIndex = indexOf()

  await screen.getByRole('button', { name: 'jump' }).click()
  await expect.element(screen.getByRole('button', { name: back })).toBeVisible()
  expect(router.state.location.pathname).toBe('/b')
  expect(router.state.location.state.returnTo).toEqual({
    page: 'rules',
    href: '/a',
    index: originIndex,
  })

  await screen.getByRole('button', { name: 'first' }).click()
  await screen.getByRole('button', { name: 'second' }).click()
  await expect.poll(() => router.state.location.search).toEqual({ n: 2 })
  expect(indexOf()).toBe(originIndex + 3)
  await expect.element(screen.getByRole('button', { name: back })).toBeVisible()

  await screen.getByRole('button', { name: back }).click()
  await expect.poll(() => router.state.location.pathname).toBe('/a')
  // Writing the focus replaced the origin entry rather than adding one.
  expect(indexOf()).toBe(originIndex)
  expect(router.state.location.state.focus).toBe('row-7')
  await expect
    .element(screen.getByTestId('origin-focus'))
    .toHaveTextContent('row-7')
  expect(screen.getByRole('button', { name: back }).query()).toBeNull()
})
