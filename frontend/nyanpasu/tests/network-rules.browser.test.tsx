import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import NetworkRuleList from '../src/pages/(main)/main/settings/system/proxy/network-rules/_modules/network-rule-list'

const backend = vi.hoisted(() => ({
  save: vi.fn(),
}))

vi.mock('@nyanpasu/platform/react', () => ({
  PlatformLinux: () => null,
  usePlatform: () => ({ surface: 'desktop', clientOs: 'linux' }),
}))
vi.mock('@nyanpasu/query', () => ({
  useNetworkAutomation: () => ({
    policy: {
      enabled: true,
      default_actions: {},
      rules: [
        {
          id: 'home',
          name: 'Home',
          enabled: true,
          conditions: { wifi_ssid: 'Home' },
          actions: { system_proxy: 'disable' },
        },
        {
          id: 'office',
          name: 'Office',
          enabled: true,
          conditions: { gateway_mac: 'aa:bb:cc:dd:ee:ff' },
          actions: { tun: 'enable' },
        },
      ],
    },
    save: backend.save,
    isPending: false,
  }),
  useNetworkAutomationStatus: () => ({
    requestWifiName: { mutateAsync: vi.fn(), isPending: false },
    query: {
      data: {
        supported: true,
        applied: null,
        network: {
          state: 'online',
          network: {
            interface_kind: 'wifi',
            wifi: { kind: 'known', ssid: 'Cafe' },
            gateway_mac: '11:22:33:44:55:66',
          },
        },
      },
    },
  }),
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('../src/pages/(main)/main/settings/_modules/open-external', () => ({
  openExternal: vi.fn(),
}))

test('deleting a rule saves the policy without it, keeping the others in order', async ({
  onTestFinished,
}) => {
  backend.save.mockResolvedValue(undefined)
  const view = await render(<NetworkRuleList />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: m.settings_system_proxy_network_automation_rule_delete(),
    })
    .first()
    .click()

  await expect.poll(() => backend.save.mock.calls.length).toBe(1)
  const saved = backend.save.mock.calls[0][0]
  expect(saved.enabled).toBe(true)
  expect(saved.rules.map((rule: { id: string }) => rule.id)).toEqual(['office'])
})

test('a rule created from the current network keeps its identifiers and saves last', async ({
  onTestFinished,
}) => {
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
  await view.getByRole('textbox').first().fill('Cafe')
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => backend.save.mock.calls.length).toBe(1)
  const rules = backend.save.mock.calls[0][0].rules
  expect(rules.map((rule: { name: string }) => rule.name)).toEqual([
    'Home',
    'Office',
    'Cafe',
  ])
  expect(rules[2].conditions).toEqual({
    wifi_ssid: 'Cafe',
    gateway_mac: '11:22:33:44:55:66',
    interface_kind: 'wifi',
  })
})
