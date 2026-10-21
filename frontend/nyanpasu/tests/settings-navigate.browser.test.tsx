import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import SettingsNavigate from '../src/pages/(main)/main/settings/_modules/settings-navigate'

vi.mock('@/hooks/use-current-core-icon', () => ({ default: () => '' }))
vi.mock('@/assets/image/logo.svg?react', () => ({ default: () => null }))

test('a settings section stays selected on its child pages', async () => {
  const rootRoute = createRootRoute({ component: SettingsNavigate })
  const creditsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/settings/about/credits',
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([creditsRoute]),
    history: createMemoryHistory({
      initialEntries: ['/main/settings/about/credits'],
    }),
  })
  const view = await render(<RouterProvider router={router} />)

  const about = view.getByRole('link', {
    name: m.settings_label_about_description(),
    exact: false,
  })
  await expect.element(about).toHaveAttribute('data-active', 'true')
  await expect
    .element(
      view.getByRole('link', {
        name: m.settings_label_debug_description(),
        exact: false,
      }),
    )
    .toHaveAttribute('data-active', 'false')
})
