import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { DnsFeatures, DnsKey } from '@nyanpasu/rpc/types'
import {
  DnsDraftProvider,
  type DnsDraft,
} from '../src/pages/(main)/main/settings/clash/dns/_modules/dns-draft'
import DnsGeneralSection from '../src/pages/(main)/main/settings/clash/dns/_modules/dns-general-section'
import DnsSaveBar from '../src/pages/(main)/main/settings/clash/dns/_modules/dns-save-bar'

const backend = vi.hoisted(() => ({
  settings: {} as Record<string, unknown>,
  save: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useClashSystemSettings: () => ({
    upsert: { mutateAsync: backend.save, isPending: false },
  }),
  useClashSetting: (key: string) => ({ value: backend.settings[key] }),
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

const ALL_KEYS: DnsKey[] = [
  'enable',
  'listen',
  'ipv6',
  'enhanced-mode',
  'fake-ip-range',
  'fake-ip-range6',
  'fake-ip-filter-mode',
  'fake-ip-filter',
  'use-hosts',
  'use-system-hosts',
  'respect-rules',
  'prefer-h3',
  'default-nameserver',
  'nameserver',
  'fallback',
  'proxy-server-nameserver',
  'direct-nameserver',
  'direct-nameserver-follow-policy',
  'nameserver-policy',
  'fallback-filter',
]

const MIHOMO: DnsFeatures = {
  keys: { kind: 'values', value: ALL_KEYS },
  enhanced_mode: { kind: 'values', value: ['normal', 'fake-ip', 'redir-host'] },
  fake_ip_filter_mode: {
    kind: 'values',
    value: ['blacklist', 'whitelist', 'rule'],
  },
  list_values: true,
  fallback_filter_geosite: true,
}

const CLASH_RS: DnsFeatures = {
  ...MIHOMO,
  keys: {
    kind: 'values',
    value: ALL_KEYS.filter(
      (key) =>
        !['use-system-hosts', 'prefer-h3', 'fake-ip-filter-mode'].includes(key),
    ),
  },
  fake_ip_filter_mode: { kind: 'ignored' },
  list_values: false,
}

const SAVED: DnsDraft = {
  dns: {
    enable: true,
    listen: null,
    ipv6: false,
    'enhanced-mode': 'fake-ip',
    'fake-ip-range': '198.18.0.1/16',
    'fake-ip-range6': null,
    'fake-ip-filter-mode': 'blacklist',
    'fake-ip-filter': ['+.lan'],
    'use-hosts': false,
    'use-system-hosts': false,
    'respect-rules': false,
    'prefer-h3': false,
    'default-nameserver': ['tls://223.5.5.5'],
    nameserver: ['https://doh.pub/dns-query'],
    fallback: [],
    'proxy-server-nameserver': [],
    'direct-nameserver': [],
    'direct-nameserver-follow-policy': false,
    'nameserver-policy': {},
    'fallback-filter': {
      geoip: true,
      'geoip-code': 'CN',
      ipcidr: [],
      domain: [],
      geosite: [],
    },
    extra: {},
  },
  hosts: {},
}

function setup(
  features: DnsFeatures,
  settings: Record<string, unknown>,
  onTestFinished: (fn: () => Promise<void>) => void,
) {
  backend.settings = settings
  backend.save.mockResolvedValue(undefined)
  onTestFinished(async () => {
    vi.clearAllMocks()
  })

  return render(
    <DnsDraftProvider saved={SAVED} features={features}>
      <DnsGeneralSection />

      <DnsSaveBar />
    </DnsDraftProvider>,
  )
}

test('saves the edited draft as one patch of dns and hosts', async ({
  onTestFinished,
}) => {
  const view = await setup(MIHOMO, {}, onTestFinished)
  onTestFinished(() => view.unmount())

  const save = view.getByRole('button', { name: m.settings_clash_dns_save() })
  await expect.element(save).toBeDisabled()

  await view
    .getByRole('switch', { name: m.settings_clash_dns_respect_rules_label() })
    .click()
  await save.click()

  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([
      {
        dns: { ...SAVED.dns, 'respect-rules': true },
        hosts: {},
      },
    ])
})

test('shows only the keys the selected core reads', async ({
  onTestFinished,
}) => {
  const view = await setup(CLASH_RS, {}, onTestFinished)
  onTestFinished(() => view.unmount())

  await expect
    .element(
      view.getByRole('switch', {
        name: m.settings_clash_dns_respect_rules_label(),
      }),
    )
    .toBeInTheDocument()
  expect(
    view
      .getByRole('switch', {
        name: m.settings_clash_dns_use_system_hosts_label(),
      })
      .elements(),
  ).toHaveLength(0)
  expect(
    view.getByText(m.settings_clash_dns_fake_ip_filter_mode_label()).elements(),
  ).toHaveLength(0)
})

test('keeps DNS on while TUN is on', async ({ onTestFinished }) => {
  const view = await setup(MIHOMO, { enable_tun_mode: true }, onTestFinished)
  onTestFinished(() => view.unmount())

  const enable = view.getByRole('switch', {
    name: m.settings_clash_dns_enable_label(),
  })
  await expect.element(enable).toBeChecked()
  await expect.element(enable).toBeDisabled()
})

test('a YAML edit fills the draft, keys without a field included', async ({
  onTestFinished,
}) => {
  const view = await setup(MIHOMO, {}, onTestFinished)
  onTestFinished(() => view.unmount())

  await view
    .getByRole('button', { name: m.settings_clash_dns_yaml_open() })
    .click()
  const editor = view.getByRole('textbox', {
    name: m.settings_clash_dns_yaml_title(),
  })
  await editor.fill(
    'dns:\n  enable: false\n  cache-algorithm: arc\nhosts:\n  router.lan: 192.168.1.1\n',
  )
  await view
    .getByRole('button', { name: m.settings_clash_dns_yaml_apply() })
    .click()
  await view.getByRole('button', { name: m.settings_clash_dns_save() }).click()

  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([
      {
        dns: {
          ...SAVED.dns,
          enable: false,
          extra: { 'cache-algorithm': 'arc' },
        },
        hosts: { 'router.lan': '192.168.1.1' },
      },
    ])
})

test('a YAML value of the wrong shape is refused', async ({
  onTestFinished,
}) => {
  const view = await setup(MIHOMO, {}, onTestFinished)
  onTestFinished(() => view.unmount())

  await view
    .getByRole('button', { name: m.settings_clash_dns_yaml_open() })
    .click()
  await view
    .getByRole('textbox', { name: m.settings_clash_dns_yaml_title() })
    .fill('dns:\n  nameserver: 1.1.1.1\n')
  await view
    .getByRole('button', { name: m.settings_clash_dns_yaml_apply() })
    .click()

  await expect
    .element(
      view.getByText(
        m.settings_clash_dns_yaml_wrong_shape({
          key: 'nameserver',
          shape: 'list',
        }),
      ),
    )
    .toBeInTheDocument()
})
