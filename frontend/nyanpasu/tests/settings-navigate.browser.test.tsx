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

vi.mock('@/assets/image/logo.svg?react', () => ({ default: () => null }))

const renderAt = async (path: string) => {
  const rootRoute = createRootRoute({ component: SettingsNavigate })
  const pageRoute = createRoute({
    getParentRoute: () => rootRoute,
    path,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([pageRoute]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  return render(<RouterProvider router={router} />)
}

const item = (
  view: Awaited<ReturnType<typeof renderAt>>,
  label: string | RegExp,
) => view.getByRole('link', { name: label, exact: false })

test('the settings navigation groups its items under headings', async () => {
  const view = await renderAt('/main/settings/debug')

  for (const heading of [
    m.settings_group_system(),
    m.settings_group_clash(),
    m.settings_group_nyanpasu(),
    m.settings_group_about(),
  ]) {
    await expect
      .element(view.getByRole('heading', { name: heading, exact: true }))
      .toBeInTheDocument()
  }

  await expect
    .element(view.getByRole('group', { name: m.settings_group_nyanpasu() }))
    .toBeInTheDocument()
})

test('an item stays selected on its child pages', async () => {
  const view = await renderAt('/main/settings/debug/child')

  const debug = item(view, m.settings_label_debug_description())
  await expect.element(debug).toHaveAttribute('data-active', 'true')
  await expect.element(debug).toHaveAttribute('aria-current', 'page')
  await expect
    .element(item(view, m.settings_label_proxy_description()))
    .toHaveAttribute('data-active', 'false')
})

test('the licenses page selects only the licenses item', async () => {
  const view = await renderAt('/main/settings/about/credits')

  await expect
    .element(item(view, m.settings_label_licenses_description()))
    .toHaveAttribute('aria-current', 'page')
  // About matches exactly, so the page under its path does not select it.
  await expect
    .element(item(view, /^Clash Nyanpasu/))
    .toHaveAttribute('data-active', 'false')
})
