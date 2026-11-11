import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import TransparentProxyConfigSettings from '../src/pages/(main)/main/settings/clash/network/_modules/transparent-proxy-config'

const backend = vi.hoisted(() => ({
  upsert: vi.fn(),
  invalidate: vi.fn(),
  message: vi.fn(),
}))

vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({
    value: key === 'core' ? 'mihomo' : true,
  }),
  useClashSetting: (key: string) => ({
    value:
      key === 'transparent_proxy'
        ? { mode: 'disabled', local: true, interfaces: ['eth0'], ipv6: false }
        : null,
    upsert: backend.upsert,
    isPending: false,
  }),
  useQueryApi: () => ({
    queries: {
      getTransparentProxyStatus: () => ({
        queryKey: ['getTransparentProxyStatus'],
        queryFn: () => Promise.resolve(),
      }),
    },
  }),
  unwrapQueryOptions: (options: unknown) => options,
}))
vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({
    data: { supported: true, active: false, error: null },
    isPending: false,
    isError: false,
  }),
  useQueryClient: () => ({ invalidateQueries: backend.invalidate }),
}))
vi.mock('@/utils/notification', () => ({ message: backend.message }))
vi.mock(
  '../src/pages/(main)/main/settings/clash/network/_modules/proxy-port-config',
  () => ({
    OptionalPortConfig: () => null,
  }),
)

test('applies normalized interfaces and discards edits on close', async ({
  onTestFinished,
}) => {
  backend.upsert.mockReset().mockResolvedValue(undefined)
  backend.invalidate.mockReset().mockResolvedValue(undefined)
  const view = await render(<TransparentProxyConfigSettings />)
  onTestFinished(() => view.unmount())

  const row = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_interfaces_label(),
    exact: false,
  })
  await expect.element(view.getByText('eth0')).toBeInTheDocument()
  await row.click()
  const input = view.getByRole('textbox', {
    name: m.settings_clash_transparent_proxy_interfaces_label(),
  })
  await input.fill(' wlan0, eth0, wlan0,  ')
  await view.getByRole('button', { name: m.common_close() }).click()
  expect(backend.upsert).not.toHaveBeenCalled()

  await row.click()
  await expect.element(input).toHaveValue('eth0')
  await input.fill(' wlan0, eth0, wlan0,  ')
  await view.getByRole('button', { name: m.common_apply() }).click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(1)
  expect(backend.upsert).toHaveBeenCalledWith({
    mode: 'disabled',
    local: true,
    interfaces: ['wlan0', 'eth0'],
    ipv6: false,
  })
})

test('keeps the dialog and edited value when saving fails', async ({
  onTestFinished,
}) => {
  backend.upsert.mockReset().mockRejectedValue(new Error('failed'))
  backend.invalidate.mockReset().mockResolvedValue(undefined)
  backend.message.mockReset()
  const view = await render(<TransparentProxyConfigSettings />)
  onTestFinished(() => view.unmount())

  await view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_interfaces_label(),
      exact: false,
    })
    .click()
  const input = view.getByRole('textbox', {
    name: m.settings_clash_transparent_proxy_interfaces_label(),
  })
  await input.fill('wlan0')
  await view.getByRole('button', { name: m.common_apply() }).click()
  await expect.poll(() => backend.message.mock.calls.length).toBe(1)
  await expect.element(input).toHaveValue('wlan0')
})
