import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import { QueryClient } from '@tanstack/react-query'
import {
  createTestRpc,
  rpcWrapper,
  type RpcHandler,
} from '../../query/tests/rpc-test-utils'
import AllowLanSwitch from '../src/pages/(main)/main/settings/clash/network/_modules/allow-lan-switch'
import { AllPermissions, withPermissions } from './all-permissions'

const mocks = vi.hoisted(() => ({
  copy: vi.fn(async () => undefined),
  message: vi.fn(async () => undefined),
}))

vi.mock('@/utils/clipboard', () => ({ writeClipboardText: mocks.copy }))
vi.mock('@/utils/notification', () => ({ message: mocks.message }))

const networks = [
  { interface: 'Ethernet', address: '192.168.1.20', network: '192.168.1.0/24' },
  {
    interface: 'Tailscale',
    address: '100.101.102.103',
    network: '100.64.0.0/10',
  },
]

const defaultLiveConfig = {
  'allow-lan': true,
  'external-controller': '0.0.0.0:9090',
  'mixed-port': 7890,
  'socks-port': 7891,
  port: 7892,
}

async function mount(
  onTestFinished: TestContext['onTestFinished'],
  handlers: Record<string, RpcHandler> = {},
  wrapper: typeof AllPermissions = AllPermissions,
) {
  const listNetworks = vi.fn(() => networks)
  const testRpc = createTestRpc({
    clash_api_get_settings: () => defaultLiveConfig,
    list_local_networks: listNetworks,
    get_clash_settings: () => ({
      clash_control_channel: 'prefer_ipc',
      clash_ipc_disable_http_controller: false,
      external_controller: {
        host: '0.0.0.0',
        port: { kind: 'fixed', start_port: 9090 },
      },
    }),
    get_clash_info: () => ({
      port: 7890,
      server: '127.0.0.1:9090',
      secret: null,
    }),
    ...handlers,
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const view = await render(
    <Wrapper>
      <AllowLanSwitch />
    </Wrapper>,
    { wrapper },
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllMocks()
  })

  return { view, listNetworks }
}

test('lists controller, mixed, socks5 and http addresses per LAN IP and copies them', async ({
  onTestFinished,
}) => {
  const { view, listNetworks } = await mount(onTestFinished)

  const button = view.getByRole('button', {
    name: m.settings_clash_settings_allow_lan_addresses_button(),
  })
  await expect.element(button).toBeVisible()
  expect(listNetworks).not.toHaveBeenCalled()

  await button.click()

  await expect
    .element(
      view.getByText(m.settings_clash_settings_allow_lan_addresses_title()),
    )
    .toBeVisible()
  expect(listNetworks).toHaveBeenCalledOnce()

  for (const address of [
    'http://192.168.1.20:9090',
    '192.168.1.20:7890',
    'socks5://192.168.1.20:7891',
    'http://192.168.1.20:7892',
    'http://100.101.102.103:9090',
    '100.101.102.103:7890',
    'socks5://100.101.102.103:7891',
    'http://100.101.102.103:7892',
  ]) {
    await expect.element(view.getByText(address, { exact: true })).toBeVisible()
  }

  await view
    .getByRole('button', {
      name: `${m.common_copy()} socks5://192.168.1.20:7891`,
    })
    .click()
  expect(mocks.copy).toHaveBeenCalledWith('socks5://192.168.1.20:7891')
})

test('skips disabled ports and notices when allow-lan is off or controller is loopback-only', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, {
    clash_api_get_settings: () => ({
      'allow-lan': false,
      'external-controller': '127.0.0.1:9090',
      'mixed-port': 7890,
      'socks-port': 0,
      port: null,
    }),
  })

  await view
    .getByRole('button', {
      name: m.settings_clash_settings_allow_lan_addresses_button(),
    })
    .click()

  await expect
    .element(
      view.getByText(
        m.settings_clash_settings_allow_lan_addresses_disabled_notice(),
      ),
    )
    .toBeVisible()
  await expect
    .element(
      view.getByText(
        m.settings_clash_settings_allow_lan_addresses_controller_loopback_notice(
          { address: '127.0.0.1:9090' },
        ),
      ),
    )
    .toBeVisible()
  await expect
    .element(view.getByText('http://192.168.1.20:9090', { exact: true }))
    .toBeVisible()
  await expect
    .element(view.getByText('192.168.1.20:7890', { exact: true }))
    .toBeVisible()
  expect(
    view.getByText(m.settings_clash_settings_socks_port_label()).query(),
  ).toBeNull()
  expect(
    view.getByText(m.settings_clash_settings_http_port_label()).query(),
  ).toBeNull()
})

test('skips controller port when HTTP controller is disabled and live config omits it', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, {
    clash_api_get_settings: () => ({
      'allow-lan': true,
      'external-controller': null,
      'mixed-port': 7890,
      'socks-port': null,
      port: null,
    }),
    get_clash_settings: () => ({
      clash_control_channel: 'prefer_ipc',
      clash_ipc_disable_http_controller: true,
      external_controller: {
        host: '127.0.0.1',
        port: { kind: 'fixed', start_port: 9090 },
      },
    }),
  })

  await view
    .getByRole('button', {
      name: m.settings_clash_settings_allow_lan_addresses_button(),
    })
    .click()

  await expect
    .element(view.getByText('192.168.1.20:7890', { exact: true }))
    .toBeVisible()
  expect(
    view
      .getByText(
        m.settings_clash_settings_allow_lan_addresses_port_controller(),
      )
      .query(),
  ).toBeNull()
})

test('shows empty state when no LAN networks are present', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, {
    list_local_networks: () => [],
  })

  await view
    .getByRole('button', {
      name: m.settings_clash_settings_allow_lan_addresses_button(),
    })
    .click()

  await expect
    .element(
      view.getByText(
        m.settings_clash_settings_allow_lan_addresses_empty_networks(),
      ),
    )
    .toBeVisible()
})

test('hides the LAN addresses button for browser callers without remote:read', async ({
  onTestFinished,
}) => {
  const { view, listNetworks } = await mount(
    onTestFinished,
    {},
    withPermissions({ settings: 'write', system: 'write' }),
  )

  await expect
    .element(view.getByText(m.settings_clash_settings_allow_lan_label()))
    .toBeVisible()
  expect(
    view
      .getByRole('button', {
        name: m.settings_clash_settings_allow_lan_addresses_button(),
      })
      .query(),
  ).toBeNull()
  expect(listNetworks).not.toHaveBeenCalled()
})
