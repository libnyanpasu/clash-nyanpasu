import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import type {
  AccessTokenSummary,
  BrowserSessionSummary,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createTestRpc,
  rpcWrapper,
  type RpcHandler,
} from '../../query/tests/rpc-test-utils'
import { Route } from '../src/pages/(main)/main/settings/nyanpasu/remote-access'
import AccessTokens from '../src/pages/(main)/main/settings/nyanpasu/remote-access/_modules/access-tokens'
import BrowserSessions from '../src/pages/(main)/main/settings/nyanpasu/remote-access/_modules/browser-sessions'
import ServerSettings from '../src/pages/(main)/main/settings/nyanpasu/remote-access/_modules/server-settings'
import { AllPermissions, withPermissions } from './all-permissions'

const dialogs = vi.hoisted(() => ({
  ask: vi.fn(async () => true),
  message: vi.fn(async () => undefined),
  copy: vi.fn(async () => undefined),
}))
vi.mock('@/utils/notification', () => ({
  ask: dialogs.ask,
  message: dialogs.message,
}))
vi.mock('@/utils/clipboard', () => ({ writeClipboardText: dialogs.copy }))
vi.mock('@/components/page-title', () => ({ PageTitle: () => null }))

const off = { enabled: false, url: null, port: null, fault: null }
const on = {
  enabled: true,
  url: 'http://127.0.0.1:12345/',
  port: 12345,
  fault: null,
}
const committed = {
  status: 'committed',
  value: null,
  commits: [],
  notifications_pending: false,
}

type HttpConfig = Record<string, unknown>

const networks = [
  { interface: 'Ethernet', address: '192.168.1.20', network: '192.168.1.0/24' },
  {
    interface: 'Tailscale',
    address: '100.101.102.103',
    network: '100.64.0.0/10',
  },
]

async function mount(
  onTestFinished: TestContext['onTestFinished'],
  ui: React.ReactNode,
  handlers: Record<string, RpcHandler> = {},
  http: HttpConfig = { enabled: false, port: null },
  wrapper: typeof AllPermissions = AllPermissions,
) {
  const state = { status: off as typeof off | typeof on, http }
  const saved = vi.fn()
  const testRpc = createTestRpc({
    get_http_server_status: () => state.status,
    get_app_settings: () => ({ remote_access: { http: state.http } }),
    set_remote_access: (params) => {
      saved(params)
      state.http = (params?.config as { http: HttpConfig }).http
      state.status = state.http.enabled ? on : off
      return committed
    },
    create_http_launch_url: () => 'http://127.0.0.1:12345/?code=once',
    open_web_url: () => null,
    list_remote_access_tokens: () => [],
    list_remote_browser_sessions: () => [],
    list_local_networks: () => networks,
    get_remote_access_certificate: () => null,
    open_remote_access_certificate: () => null,
    export_remote_access_authority: () => true,
    ...handlers,
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const view = await render(<Wrapper>{ui}</Wrapper>, { wrapper })
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllMocks()
  })

  return { view, testRpc, saved }
}

const enabledSwitch = (view: Awaited<ReturnType<typeof render>>) =>
  view.getByRole('switch', { name: m.settings_remote_access_enabled_title() })

test('the switch writes the whole remote access section and shows the origin', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(onTestFinished, <ServerSettings />)
  const toggle = enabledSwitch(view)
  await expect.element(toggle).toBeEnabled()

  await toggle.click()

  await expect.element(toggle).toBeChecked()
  await expect
    .element(view.getByText('http://127.0.0.1:12345/', { exact: true }))
    .toBeVisible()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: null } },
  })

  await toggle.click()
  await expect.element(toggle).not.toBeChecked()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: false, port: null } },
  })
})

test('the launch URL is requested on demand and handed to the opener only', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(onTestFinished, <ServerSettings />)
  const toggle = enabledSwitch(view)
  await expect.element(toggle).toBeEnabled()
  await toggle.click()
  await expect.element(toggle).toBeChecked()
  expect(testRpc.invoke).not.toHaveBeenCalledWith(
    'create_http_launch_url',
    expect.anything(),
  )

  await view
    .getByRole('button', { name: m.settings_remote_access_open() })
    .click()

  await vi.waitFor(() =>
    expect(testRpc.invoke).toHaveBeenCalledWith('open_web_url', {
      url: 'http://127.0.0.1:12345/?code=once',
    }),
  )
  expect(view.container.textContent).not.toContain('code=once')
})

test('a taken port is reported while the saved setting stays on and can be turned off', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    { get_http_server_status: () => ({ ...off, fault: 'port_in_use' }) },
    { enabled: true, port: 8765 },
  )

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.settings_remote_access_fault_port_in_use())
  // The committed desire is on although nothing listens, and nothing is rolled back.
  const toggle = enabledSwitch(view)
  await expect.element(toggle).toBeChecked()

  await toggle.click()

  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: false, port: 8765 } },
  })
})

test('a failed change reports the error', async ({ onTestFinished }) => {
  const { view } = await mount(onTestFinished, <ServerSettings />, {
    set_remote_access: () => {
      throw new Error('commit failed')
    },
  })
  const toggle = enabledSwitch(view)
  await expect.element(toggle).toBeEnabled()

  await toggle.click()

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(
      'The operation result is unconfirmed. Inspect its current status before trying again.',
    )
})

test('the web interface switch and the port are saved without losing the rest', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, serve_webui: true },
  )
  const webui = view.getByRole('switch', {
    name: m.settings_remote_access_serve_webui_title(),
  })
  await expect.element(webui).toBeEnabled()

  await webui.click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: 8765, serve_webui: false } },
  })

  // The row shows the saved port and opens a dialog with one field.
  await view
    .getByRole('button', {
      name: `${m.settings_remote_access_port_title()} 8765`,
    })
    .click()
  const dialog = view.getByRole('dialog')
  const port = dialog.getByRole('textbox', {
    name: m.settings_remote_access_port_title(),
  })
  await expect.element(port).toHaveValue('8765')

  await port.fill('9000')
  await dialog.getByRole('button', { name: m.common_apply() }).click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: 9000, serve_webui: false } },
  })
  await expect.element(dialog).not.toBeInTheDocument()

  // An empty field leaves the choice to the server's first start.
  await view
    .getByRole('button', {
      name: `${m.settings_remote_access_port_title()} 9000`,
    })
    .click()
  await view.getByRole('dialog').getByRole('textbox').fill('')
  await view
    .getByRole('dialog')
    .getByRole('button', { name: m.common_apply() })
    .click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: null, serve_webui: false } },
  })
})

const choice = (
  view: Awaited<ReturnType<typeof render>>,
  group: () => string,
  option: () => string,
) =>
  view.getByRole('radiogroup', { name: group() }).getByRole('radio', {
    name: option(),
  })

test('optional authentication warns and offers anonymous permissions up to the operator role', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, auth: 'required', listen: 'local' },
  )
  expect(
    view.getByText(m.settings_remote_access_auth_optional_warning()).query(),
  ).toBeNull()

  await choice(
    view,
    m.settings_remote_access_auth_title,
    m.settings_remote_access_auth_optional,
  ).click()

  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: { enabled: true, port: 8765, auth: 'optional', listen: 'local' },
    },
  })
  await expect
    .element(view.getByText(m.settings_remote_access_auth_optional_warning()))
    .toBeVisible()

  const row = (resource: () => string) =>
    view.getByRole('radiogroup', { name: resource() })
  const level = (resource: () => string, label: () => string) =>
    row(resource).getByRole('radio', { name: label() })

  // Nothing is granted by default, remote access is never offered, and the
  // system and logs levels stop at what the operator role reads.
  await expect
    .element(
      level(
        m.settings_remote_access_resource_proxy,
        m.settings_remote_access_access_none,
      ),
    )
    .toBeChecked()
  expect(row(m.settings_remote_access_resource_remote).query()).toBeNull()
  for (const resource of [
    m.settings_remote_access_resource_system,
    m.settings_remote_access_resource_logs,
  ]) {
    await expect
      .element(level(resource, m.settings_remote_access_access_write))
      .toBeDisabled()
    await expect
      .element(level(resource, m.settings_remote_access_access_read))
      .toBeEnabled()
  }

  await level(
    m.settings_remote_access_resource_proxy,
    m.settings_remote_access_access_write,
  ).click()
  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: {
        enabled: true,
        port: 8765,
        auth: 'optional',
        listen: 'local',
        anonymous_permissions: { proxy: 'write' },
      },
    },
  })
})

const lan = {
  enabled: true,
  port: 8765,
  auth: 'required',
  listen: 'lan',
  tls: 'self_signed',
  lan_allow: ['192.168.1.0/24'],
}

const fingerprint =
  'AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89'

test('the LAN scope brings HTTPS and authentication, allows the private networks and warns', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, auth: 'optional', listen: 'local' },
  )
  const https = view.getByRole('switch', {
    name: m.settings_remote_access_tls_title(),
  })
  await expect.element(https).not.toBeChecked()
  const lanScope = choice(
    view,
    m.settings_remote_access_listen_title,
    m.settings_remote_access_listen_lan,
  )
  await expect.element(lanScope).toBeEnabled()

  await lanScope.click()

  // One write carries every field, so the committed value never leaves this computer without
  // HTTPS or with anonymous access.
  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: {
        enabled: true,
        port: 8765,
        auth: 'required',
        listen: 'lan',
        tls: 'self_signed',
        lan_allow: ['192.168.1.0/24'],
      },
    },
  })
  await expect
    .element(view.getByText(m.settings_remote_access_listen_lan_warning()))
    .toBeVisible()
  await expect.element(https).toBeChecked()
  await expect.element(https).toBeDisabled()
  await expect
    .element(
      choice(
        view,
        m.settings_remote_access_auth_title,
        m.settings_remote_access_auth_optional,
      ),
    )
    .toBeDisabled()
})

test('every network is reachable only after choosing it, with its own warning', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    lan,
  )

  await choice(
    view,
    m.settings_remote_access_listen_title,
    m.settings_remote_access_listen_any,
  ).click()

  // The allow list is kept for a later return to the LAN scope.
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { ...lan, listen: 'any' } },
  })
  await expect
    .element(view.getByText(m.settings_remote_access_listen_any_warning()))
    .toBeVisible()
  expect(
    view.getByText(m.settings_remote_access_lan_allow_title()).query(),
  ).toBeNull()
})

test('allowed networks are toggled and the last one cannot be removed', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { ...lan, lan_allow: ['192.168.1.0/24', '10.147.17.0/24'] },
  )
  const network = (name: string) =>
    view.getByRole('checkbox', { name, exact: false })

  // An allowed network that is not connected now stays listed so it can be removed.
  await expect
    .element(view.getByText(m.settings_remote_access_lan_allow_disconnected()))
    .toBeVisible()
  await expect.element(network('100.64.0.0/10')).not.toBeChecked()

  await network('100.64.0.0/10').click()
  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: {
        ...lan,
        lan_allow: ['192.168.1.0/24', '10.147.17.0/24', '100.64.0.0/10'],
      },
    },
  })

  await network('10.147.17.0/24').click()
  await network('100.64.0.0/10').click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { ...lan, lan_allow: ['192.168.1.0/24'] } },
  })
  await expect.element(network('192.168.1.0/24')).toBeDisabled()
})

test('the LAN scope cannot be chosen while this computer has no network', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    { list_local_networks: () => [] },
    { enabled: true, port: 8765, listen: 'local', lan_allow: [] },
  )

  await expect
    .element(
      choice(
        view,
        m.settings_remote_access_listen_title,
        m.settings_remote_access_listen_lan,
      ),
    )
    .toBeDisabled()
})

test('HTTPS can be chosen on its own for the loopback scope', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, listen: 'local', tls: 'http' },
  )
  const https = view.getByRole('switch', {
    name: m.settings_remote_access_tls_title(),
  })
  await expect.element(https).toBeEnabled()

  await https.click()

  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: {
        enabled: true,
        port: 8765,
        listen: 'local',
        tls: 'self_signed',
      },
    },
  })
})

const listeningOverHttps = () => ({
  ...on,
  url: 'https://127.0.0.1:12345/',
  tls: { fingerprint, not_after_ms: Date.UTC(2028, 0, 2) },
})

test('the addresses of the allowed networks are listed at the top while the server listens', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    { get_http_server_status: listeningOverHttps },
    lan,
  )

  await expect
    .element(view.getByText(m.settings_remote_access_addresses_title()))
    .toBeVisible()
  for (const url of [
    'https://127.0.0.1:12345/',
    'https://192.168.1.20:12345/',
  ]) {
    await expect.element(view.getByText(url, { exact: true })).toBeVisible()
  }
  // Tailscale is attached but not allowed, so it is not offered as an address.
  expect(
    view.getByText('https://100.101.102.103:12345/', { exact: true }).query(),
  ).toBeNull()

  await view
    .getByRole('button', {
      name: `${m.common_copy()} https://192.168.1.20:12345/`,
    })
    .click()
  expect(dialogs.copy).toHaveBeenCalledWith('https://192.168.1.20:12345/')
})

test('every address of this computer is listed for every network', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    { get_http_server_status: listeningOverHttps },
    { ...lan, listen: 'any' },
  )

  for (const url of [
    'https://192.168.1.20:12345/',
    'https://100.101.102.103:12345/',
  ]) {
    await expect.element(view.getByText(url, { exact: true })).toBeVisible()
  }
})

test('no address is listed while the server does not listen', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, <ServerSettings />, {}, lan)

  await expect.element(enabledSwitch(view)).toBeEnabled()
  expect(
    view.getByText(m.settings_remote_access_addresses_title()).query(),
  ).toBeNull()
})

const authority = {
  subject: 'CN=Clash Nyanpasu Local CA (nyan)',
  issuer: 'CN=Clash Nyanpasu Local CA (nyan)',
  serial: '0A:0B',
  not_before_ms: Date.UTC(2026, 10, 18),
  not_after_ms: Date.UTC(2036, 10, 16),
  dns_names: [],
  ip_addresses: [],
  signature_algorithm: 'ecdsa-with-SHA256',
  public_key_algorithm: 'id-ecPublicKey',
  sha256: fingerprint,
  sha1: 'AA:BB',
}

const certificate = {
  authority,
  leaf: {
    subject: 'CN=Clash Nyanpasu',
    issuer: authority.subject,
    serial: '01:02:03',
    not_before_ms: Date.UTC(2026, 10, 18),
    not_after_ms: Date.UTC(2026, 10, 25),
    dns_names: ['localhost', 'nyan.local'],
    ip_addresses: ['127.0.0.1', '192.168.1.20'],
    signature_algorithm: 'ecdsa-with-SHA256',
    public_key_algorithm: 'id-ecPublicKey',
    sha256:
      '01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01',
    sha1: '11:22:33',
  },
}

test('the authority is summarized with the current certificate, detailed, opened and exported', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(
    onTestFinished,
    <ServerSettings />,
    // Nothing listens, yet the saved authority is shown.
    { get_remote_access_certificate: () => certificate },
    lan,
  )

  // The fingerprint to compare is the authority's, which outlives every renewal.
  await expect.element(view.getByText(fingerprint)).toBeVisible()
  await expect
    .element(view.getByText('CN=Clash Nyanpasu Local CA (nyan)'))
    .toBeVisible()
  await expect
    .element(
      view.getByText(
        m.settings_remote_access_certificate_leaf_summary({
          date: formatDate(certificate.leaf.not_after_ms),
        }),
      ),
    )
    .toBeVisible()

  await view
    .getByRole('button', {
      name: m.settings_remote_access_certificate_export(),
    })
    .click()
  await vi.waitFor(() =>
    expect(testRpc.invoke).toHaveBeenCalledWith(
      'export_remote_access_authority',
      undefined,
    ),
  )

  await view
    .getByRole('button', {
      name: m.settings_remote_access_certificate_details(),
    })
    .click()
  const dialog = view.getByRole('dialog')
  // The authority signs itself; the current certificate names the addresses it covers.
  await expect
    .element(
      dialog.getByText(
        `CN=Clash Nyanpasu Local CA (nyan) (${m.settings_remote_access_certificate_self_signed()})`,
      ),
    )
    .toBeVisible()
  await expect.element(dialog.getByText('01:02:03')).toBeVisible()
  await expect.element(dialog.getByText('localhost, nyan.local')).toBeVisible()
  await expect.element(dialog.getByText('11:22:33')).toBeVisible()

  await dialog
    .getByRole('button', {
      name: m.settings_remote_access_certificate_open_viewer(),
    })
    .click()
  await vi.waitFor(() =>
    expect(testRpc.invoke).toHaveBeenCalledWith(
      'open_remote_access_certificate',
      undefined,
    ),
  )
})

test('the certificate fingerprint is shown and replacing it asks first', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      get_http_server_status: listeningOverHttps,
      get_remote_access_certificate: () => certificate,
      reset_remote_access_tls: () => committed,
    },
    lan,
  )
  await expect.element(view.getByText(fingerprint)).toBeVisible()

  dialogs.ask.mockResolvedValueOnce(false)
  const replace = view.getByRole('button', {
    name: m.settings_remote_access_certificate_reset(),
  })
  await replace.click()
  expect(dialogs.ask).toHaveBeenLastCalledWith(
    m.settings_remote_access_certificate_reset_confirm(),
    { kind: 'warning' },
  )
  expect(testRpc.invoke).not.toHaveBeenCalledWith(
    'reset_remote_access_tls',
    expect.anything(),
  )

  await replace.click()
  await vi.waitFor(() =>
    expect(testRpc.invoke).toHaveBeenCalledWith(
      'reset_remote_access_tls',
      undefined,
    ),
  )
})

test('a certificate not made yet is explained and can be made by replacing it', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      // The listener presents nothing, so the status carries no certificate.
      get_http_server_status: () => ({ ...off, fault: 'tls_unavailable' }),
      reset_remote_access_tls: () => committed,
    },
    lan,
  )
  await expect
    .element(view.getByText(m.settings_remote_access_certificate_none()))
    .toBeVisible()
  expect(view.getByText(fingerprint).query()).toBeNull()

  await view
    .getByRole('button', {
      name: m.settings_remote_access_certificate_reset(),
    })
    .click()
  await vi.waitFor(() =>
    expect(testRpc.invoke).toHaveBeenCalledWith(
      'reset_remote_access_tls',
      undefined,
    ),
  )
})

test('no fingerprint is shown while the server does not serve HTTPS', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, <ServerSettings />, {
    get_http_server_status: () => on,
  })
  await expect
    .element(
      view.getByRole('switch', { name: m.settings_remote_access_tls_title() }),
    )
    .toBeEnabled()

  expect(
    view.getByText(m.settings_remote_access_certificate_title()).query(),
  ).toBeNull()
})

test('a certificate that cannot be prepared is reported while the setting stays on', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    { get_http_server_status: () => ({ ...off, fault: 'tls_unavailable' }) },
    lan,
  )

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.settings_remote_access_fault_tls_unavailable())
})

const token = (over: Partial<AccessTokenSummary> = {}): AccessTokenSummary => ({
  id: 't1',
  name: 'laptop',
  grant: { role: 'viewer' },
  created_at_ms: Date.UTC(2026, 0, 2, 3, 4, 5),
  expires_at_ms: null,
  last_used_at_ms: null,
  status: 'active',
  ...over,
})

test('the token is shown once and the dialog stays until it is confirmed saved', async ({
  onTestFinished,
}) => {
  const tokens: AccessTokenSummary[] = []
  const { view, testRpc } = await mount(onTestFinished, <AccessTokens />, {
    list_remote_access_tokens: () => [...tokens],
    create_remote_access_token: (params) => {
      const request = params?.request as {
        name: string
        grant: AccessTokenSummary['grant']
      }
      const summary = token({
        id: 'new',
        name: request.name,
        grant: request.grant,
      })
      tokens.push(summary)
      return { summary, token: 'secret-token-value' }
    },
  })
  await expect
    .element(view.getByText(m.settings_remote_access_tokens_empty()))
    .toBeVisible()

  await view
    .getByRole('button', { name: m.settings_remote_access_token_create() })
    .click()
  const dialog = view.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  const create = dialog.getByRole('button', {
    name: m.settings_remote_access_token_create(),
  })
  await expect.element(create).toBeDisabled()

  await dialog
    .getByRole('textbox', { name: m.settings_remote_access_token_name() })
    .fill('phone')
  // A role is chosen by default; a custom map offers every resource but remote.
  await expect
    .element(
      dialog.getByRole('radio', {
        name: m.settings_remote_access_role_viewer(),
      }),
    )
    .toBeChecked()
  await dialog
    .getByRole('radio', { name: m.settings_remote_access_role_custom() })
    .click()
  expect(
    dialog
      .getByRole('radiogroup', {
        name: m.settings_remote_access_resource_remote(),
      })
      .query(),
  ).toBeNull()
  // An empty custom map grants nothing, so it cannot be created.
  await expect.element(create).toBeDisabled()
  await dialog
    .getByRole('radiogroup', {
      name: m.settings_remote_access_resource_proxy(),
    })
    .getByRole('radio', { name: m.settings_remote_access_access_write() })
    .click()
  await dialog
    .getByRole('radiogroup', { name: m.settings_remote_access_resource_logs() })
    .getByRole('radio', { name: m.settings_remote_access_access_read() })
    .click()
  await create.click()

  await expect.element(dialog.getByText('secret-token-value')).toBeVisible()
  expect(testRpc.invoke).toHaveBeenCalledWith('create_remote_access_token', {
    request: {
      name: 'phone',
      grant: { permissions: { proxy: 'write', logs: 'read' } },
      expires_at_ms: null,
    },
  })

  // It cannot be dismissed, nor finished, before the user confirms the copy.
  const done = dialog.getByRole('button', {
    name: m.settings_remote_access_token_done(),
  })
  await expect.element(done).toBeDisabled()
  await dialog
    .getByRole('button', { name: m.settings_remote_access_token_copy() })
    .click()
  expect(dialogs.copy).toHaveBeenCalledWith('secret-token-value')
  await dialog
    .getByRole('checkbox', { name: m.settings_remote_access_token_confirm() })
    .click()
  await expect.element(done).toBeEnabled()
  await done.click()

  await expect.element(dialog).not.toBeInTheDocument()
  expect(view.container.textContent).not.toContain('secret-token-value')
  await expect.element(view.getByText('phone', { exact: false })).toBeVisible()
})

test('a token is revoked after confirmation by its id', async ({
  onTestFinished,
}) => {
  const revoked = vi.fn()
  const { view } = await mount(onTestFinished, <AccessTokens />, {
    list_remote_access_tokens: () => [
      token({ id: 't1', name: 'laptop' }),
      token({ id: 't2', name: 'old', status: 'expired' }),
    ],
    revoke_remote_access_token: (params) => {
      revoked(params)
      return null
    },
  })
  await expect.element(view.getByText('old', { exact: false })).toBeVisible()

  await view
    .getByRole('button', { name: m.settings_remote_access_token_revoke() })
    .first()
    .click()

  expect(dialogs.ask).toHaveBeenCalledWith(
    m.settings_remote_access_token_revoke_confirm({ name: 'laptop' }),
    expect.anything(),
  )
  await vi.waitFor(() => expect(revoked).toHaveBeenCalledWith({ id: 't1' }))
})

test('declining the confirmation keeps the token', async ({
  onTestFinished,
}) => {
  dialogs.ask.mockResolvedValueOnce(false)
  const revoked = vi.fn()
  const { view } = await mount(onTestFinished, <AccessTokens />, {
    list_remote_access_tokens: () => [token()],
    revoke_remote_access_token: revoked,
  })

  await view
    .getByRole('button', { name: m.settings_remote_access_token_revoke() })
    .click()

  await vi.waitFor(() => expect(dialogs.ask).toHaveBeenCalled())
  expect(revoked).not.toHaveBeenCalled()
})

test('browser sessions name their token and are revoked by id', async ({
  onTestFinished,
}) => {
  const revoked = vi.fn()
  const session = (over: Partial<BrowserSessionSummary>) => ({
    id: 's1',
    backing_token_id: null,
    grant: { role: 'admin' } as const,
    created_at_ms: Date.UTC(2026, 0, 2),
    last_active_at_ms: Date.UTC(2026, 0, 3),
    token_expires_at_ms: null,
    ...over,
  })
  const { view } = await mount(onTestFinished, <BrowserSessions />, {
    list_remote_access_tokens: () => [token({ id: 't1', name: 'laptop' })],
    list_remote_browser_sessions: () => [
      session({ id: 's1', backing_token_id: 't1' }),
      session({ id: 's2' }),
    ],
    revoke_remote_browser_session: (params) => {
      revoked(params)
      return null
    },
  })

  await expect
    .element(
      view.getByText(
        m.settings_remote_access_session_via_token({ name: 'laptop' }),
      ),
    )
    .toBeVisible()
  await expect
    .element(view.getByText(m.settings_remote_access_session_via_launch()))
    .toBeVisible()

  await view
    .getByRole('button', { name: m.settings_remote_access_session_revoke() })
    .last()
    .click()

  await vi.waitFor(() => expect(revoked).toHaveBeenCalledWith({ id: 's2' }))
})

test('the page is offered to the desktop app alone', async ({
  onTestFinished,
}) => {
  const Page = Route.options.component!
  const remote = await mount(
    onTestFinished,
    <Page />,
    {},
    { enabled: true, port: 1 },
    withPermissions({ profile: 'read', settings: 'read', remote: 'write' }),
  )

  await expect
    .element(remote.view.getByText(m.settings_remote_access_desktop_only()))
    .toBeVisible()
  expect(
    remote.view
      .getByRole('switch', { name: m.settings_remote_access_enabled_title() })
      .query(),
  ).toBeNull()
  expect(remote.testRpc.invoke).not.toHaveBeenCalledWith(
    'list_remote_access_tokens',
    expect.anything(),
  )
  await remote.view.unmount()

  const desktop = await mount(
    onTestFinished,
    <Page />,
    {},
    { enabled: false, port: null },
    AllPermissions,
  )

  await expect
    .element(
      desktop.view.getByRole('switch', {
        name: m.settings_remote_access_enabled_title(),
      }),
    )
    .toBeVisible()
})
