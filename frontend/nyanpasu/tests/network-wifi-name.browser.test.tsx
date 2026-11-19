import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import NetworkRuleList from '../src/pages/(main)/main/settings/system/proxy/network-rules/_modules/network-rule-list'
import WifiNameNotice from '../src/pages/(main)/main/settings/system/proxy/network-rules/_modules/wifi-name-notice'

const backend = vi.hoisted(() => ({
  save: vi.fn(),
  requestWifiName: vi.fn(),
  openExternal: vi.fn(),
  clientOs: 'macos' as 'macos' | 'windows',
  wifi: { kind: 'not_read' } as
    | { kind: 'not_read' }
    | { kind: 'known'; ssid: string }
    | { kind: 'unavailable'; reason: string },
}))

const network = (wifi: unknown) => ({
  state: 'online',
  network: {
    interface_kind: 'wifi',
    wifi,
    gateway_mac: '11:22:33:44:55:66',
  },
})

vi.mock('@nyanpasu/platform/react', () => ({
  PlatformLinux: () => null,
  usePlatform: () => ({ surface: 'desktop', clientOs: backend.clientOs }),
}))
vi.mock('@nyanpasu/query', () => ({
  useNetworkAutomation: () => ({
    policy: { enabled: true, default_actions: {}, rules: [] },
    save: backend.save,
    isPending: false,
  }),
  useNetworkAutomationStatus: () => ({
    query: {
      data: { supported: true, applied: null, network: network(backend.wifi) },
    },
    requestWifiName: {
      mutateAsync: backend.requestWifiName,
      isPending: false,
    },
  }),
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('../src/pages/(main)/main/settings/_modules/open-external', () => ({
  openExternal: backend.openExternal,
}))

const MACOS_LOCATION_SETTINGS =
  'x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices'
const WINDOWS_LOCATION_SETTINGS = 'ms-settings:privacy-location'

test('an undecided permission offers to ask and hands the reading to the caller', async ({
  onTestFinished,
}) => {
  const status = { supported: true, applied: null, network: null }
  backend.requestWifiName.mockResolvedValue(status)
  const onRequested = vi.fn()
  const view = await render(
    <WifiNameNotice
      wifi={{ kind: 'unavailable', reason: 'permission_required' }}
      onRequested={onRequested}
    />,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_network_automation_wifi_permission_required(),
      ),
    )
    .toBeVisible()
  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_wifi_allow(),
    })
    .click()

  await expect.poll(() => onRequested.mock.calls.length).toBe(1)
  expect(onRequested).toHaveBeenCalledWith(status)
})

test.each([
  ['macos', MACOS_LOCATION_SETTINGS],
  ['windows', WINDOWS_LOCATION_SETTINGS],
] as const)(
  'a refused permission opens the %s location settings',
  async (clientOs, url) => {
    backend.clientOs = clientOs
    const view = await render(
      <WifiNameNotice
        wifi={{ kind: 'unavailable', reason: 'permission_denied' }}
      />,
    )

    await expect
      .element(
        view.getByText(
          m.settings_system_proxy_network_automation_wifi_permission_denied(),
        ),
      )
      .toBeVisible()
    await view
      .getByRole('button', {
        name: m.settings_system_proxy_network_automation_wifi_open_settings(),
      })
      .click()

    await expect.poll(() => backend.openExternal.mock.calls.length).toBe(1)
    expect(backend.openExternal).toHaveBeenCalledWith(url)

    await view.unmount()
    vi.clearAllMocks()
  },
)

test('a name that is known or not wanted shows no notice', async ({
  onTestFinished,
}) => {
  const known = await render(
    <WifiNameNotice wifi={{ kind: 'known', ssid: 'Cafe' }} />,
  )
  expect(known.container.querySelector('[data-slot="wifi-name-notice"]')).toBe(
    null,
  )
  await known.unmount()

  const unread = await render(<WifiNameNotice wifi={{ kind: 'not_read' }} />)
  onTestFinished(() => unread.unmount())
  expect(unread.container.querySelector('[data-slot="wifi-name-notice"]')).toBe(
    null,
  )
})

test('using the current network asks for a name nobody has read yet', async ({
  onTestFinished,
}) => {
  backend.wifi = { kind: 'not_read' }
  backend.requestWifiName.mockResolvedValue({
    supported: true,
    applied: null,
    network: network({ kind: 'known', ssid: 'Cafe' }),
  })
  backend.save.mockResolvedValue(undefined)
  const view = await render(<NetworkRuleList />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_rule_add(),
    })
    .click()
  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_use_current(),
    })
    .click()
  await view.getByRole('textbox').first().fill('Cafe rule')
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => backend.save.mock.calls.length).toBe(1)
  expect(backend.requestWifiName).toHaveBeenCalledTimes(1)
  expect(backend.save.mock.calls[0][0].rules[0].conditions).toEqual({
    wifi_ssid: 'Cafe',
    gateway_mac: '11:22:33:44:55:66',
    interface_kind: 'wifi',
  })
})

test('using the current network does not ask again once the permission is refused', async ({
  onTestFinished,
}) => {
  backend.wifi = { kind: 'unavailable', reason: 'permission_denied' }
  const view = await render(<NetworkRuleList />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_rule_add(),
    })
    .click()
  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_use_current(),
    })
    .click()

  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_network_automation_wifi_permission_denied(),
      ),
    )
    .toBeVisible()
  expect(backend.requestWifiName).not.toHaveBeenCalled()
})
