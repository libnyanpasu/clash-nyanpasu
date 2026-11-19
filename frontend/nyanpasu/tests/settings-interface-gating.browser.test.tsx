import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as CustomCssRoute } from '../src/pages/(main)/main/settings/nyanpasu/interface/custom-css/index'
import { Route as PresetsRoute } from '../src/pages/(main)/main/settings/nyanpasu/interface/presets/index'
import { withPermissions } from './all-permissions'

const customCss = vi.hoisted(() => ({
  useCustomCss: vi.fn(() => {
    throw new Error('a restricted caller must not mount custom CSS')
  }),
}))

vi.mock('@/hooks/use-custom-css', () => ({
  default: customCss.useCustomCss,
  reportCustomCssError: vi.fn(),
}))

test.each([
  ['presets', PresetsRoute.options.component!],
  ['custom CSS', CustomCssRoute.options.component!],
])(
  'a caller without settings:read sees the restricted %s page without mounting it',
  async (_, Page) => {
    const root = createRootRoute({
      component: () => (
        <ScrollArea>
          <Page />
        </ScrollArea>
      ),
    })
    const router = createRouter({
      routeTree: root,
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })

    const view = await render(<RouterProvider router={router} />, {
      wrapper: withPermissions({ proxy: 'write' }),
    })

    await expect
      .element(view.getByText(m.auth_restricted_title()))
      .toBeInTheDocument()
    expect(customCss.useCustomCss).not.toHaveBeenCalled()

    await view.unmount()
  },
)
