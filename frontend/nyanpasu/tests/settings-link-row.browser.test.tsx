import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { SettingsLinkRow } from '../src/pages/(main)/main/settings/_modules/settings-link-row'

test('a settings entry row is one link with its own and shared slots', async () => {
  const root = createRootRoute({
    component: () => (
      <SettingsLinkRow
        to="/main/settings/system/proxy/guard"
        title="System Proxy Guard"
        description="Enabled"
        data-slot="proxy-guard-entry"
      />
    ),
  })
  const router = createRouter({
    routeTree: root,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  const view = await render(<RouterProvider router={router} />)

  const link = view.getByRole('link', { name: /System Proxy Guard/ })
  await expect.element(link).toBeVisible()
  expect(link.element().getAttribute('href')).toBe(
    '/main/settings/system/proxy/guard',
  )
  expect(link.element().getAttribute('data-slot')).toBe('proxy-guard-entry')
  expect(
    view.container.querySelector('[data-slot="settings-link-row"]'),
  ).not.toBeNull()

  await view.unmount()
})
