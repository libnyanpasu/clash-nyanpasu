import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import { TransparentProxyButton } from '../src/pages/(tray-menu)/tray-menu/_modules/transparent-proxy-button'

const proxy = vi.hoisted(() => ({
  visible: true,
  isActive: false,
  isPending: false,
  disabled: false,
  reason: undefined as string | undefined,
  execute: vi.fn(),
}))

vi.mock('@/hooks/use-proxy-settings', () => ({
  useTransparentProxy: () => proxy,
}))
vi.mock('../src/pages/(tray-menu)/tray-menu/_modules/hooks', () => ({
  useTrayClickHandler: (handler: () => Promise<void>) => handler,
}))

test('tray webview shows and toggles transparent capture', async ({
  onTestFinished,
}) => {
  proxy.visible = true
  proxy.disabled = false
  proxy.isActive = false
  proxy.execute.mockResolvedValue(undefined)
  const view = await render(<TransparentProxyButton />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  const button = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_label(),
    exact: false,
  })
  await expect.element(button).toHaveAttribute('data-checked', 'false')
  await button.click()
  expect(proxy.execute).toHaveBeenCalledOnce()
  proxy.isActive = true
  await view.rerender(<TransparentProxyButton />)
  await expect.element(button).toHaveAttribute('data-checked', 'true')
})

test('tray webview hides unsupported systems and blocks unavailable activation', async ({
  onTestFinished,
}) => {
  proxy.visible = true
  proxy.isActive = false
  proxy.disabled = true
  proxy.reason = 'service required'
  const view = await render(<TransparentProxyButton />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  const button = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_label(),
    exact: false,
  })
  await expect.element(button).toBeDisabled()
  await expect.element(button).toHaveAttribute('title', 'service required')
  proxy.visible = false
  await view.rerender(<TransparentProxyButton />)
  await expect.element(button).not.toBeInTheDocument()
})
