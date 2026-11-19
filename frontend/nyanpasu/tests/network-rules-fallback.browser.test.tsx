import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { PlatformProvider } from '@nyanpasu/platform/react'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as NetworkRulesRoute } from '../src/pages/(main)/main/settings/system/proxy/network-rules/index'

const query = vi.hoisted(() => ({
  useNetworkAutomation: vi.fn(() => {
    throw new Error('the browser fallback must not read network rules')
  }),
}))

vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useNetworkAutomation: query.useNetworkAutomation,
}))

test('a direct browser link shows the unsupported state without reading rules', async () => {
  const NetworkRules = NetworkRulesRoute.options.component!
  const root = createRootRoute({
    component: () => (
      <ScrollArea>
        <NetworkRules />
      </ScrollArea>
    ),
  })
  const router = createRouter({
    routeTree: root,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  const view = await render(
    <PlatformProvider
      platform={{ surface: 'browser', clientOs: 'linux' }}
      bridge="none"
    >
      <RouterProvider router={router} />
    </PlatformProvider>,
  )

  await expect
    .element(
      view.getByText(m.settings_system_proxy_network_automation_unsupported()),
    )
    .toBeInTheDocument()
  expect(query.useNetworkAutomation).not.toHaveBeenCalled()

  await view.unmount()
})
