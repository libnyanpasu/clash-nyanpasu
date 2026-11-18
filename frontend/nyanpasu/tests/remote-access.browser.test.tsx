import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
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
    .element(
      view.getByText(
        m.settings_remote_access_listening({ url: 'http://127.0.0.1:12345/' }),
      ),
    )
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

  const port = view.getByRole('textbox', {
    name: m.settings_remote_access_port_title(),
  })
  const save = view.getByRole('button', { name: m.common_save() })
  await expect.element(save).toBeDisabled()

  await port.fill('70000')
  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.settings_remote_access_port_invalid())
  await expect.element(save).toBeDisabled()

  await port.fill('9000')
  await save.click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: 9000, serve_webui: false } },
  })
})

test('local open warns and offers anonymous permissions up to the operator role', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, strategy: 'local_authenticated' },
  )
  expect(
    view
      .getByText(m.settings_remote_access_strategy_local_open_warning())
      .query(),
  ).toBeNull()

  await view
    .getByRole('button', {
      name: m.settings_remote_access_strategy_title(),
      exact: false,
    })
    .click()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_remote_access_strategy_local_open(),
      exact: false,
    })
    .click()

  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: { enabled: true, port: 8765, strategy: 'local_open' },
    },
  })
  await expect
    .element(
      view.getByText(m.settings_remote_access_strategy_local_open_warning()),
    )
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
        strategy: 'local_open',
        anonymous_permissions: { proxy: 'write' },
      },
    },
  })
})

const lan = {
  enabled: true,
  port: 8765,
  strategy: 'lan_authenticated',
  tls: 'self_signed',
  lan_bind: null,
}

const fingerprint =
  'AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89'

test('sharing on the local network brings HTTPS, warns and cannot drop HTTPS', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, strategy: 'local_authenticated' },
  )
  const https = view.getByRole('switch', {
    name: m.settings_remote_access_tls_title(),
  })
  await expect.element(https).not.toBeChecked()

  await view
    .getByRole('button', {
      name: m.settings_remote_access_strategy_title(),
      exact: false,
    })
    .click()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_remote_access_strategy_lan_authenticated(),
      exact: false,
    })
    .click()

  // One write carries both fields, so the committed value is never LAN without HTTPS.
  expect(saved).toHaveBeenLastCalledWith({
    config: {
      http: {
        enabled: true,
        port: 8765,
        strategy: 'lan_authenticated',
        tls: 'self_signed',
      },
    },
  })
  await expect
    .element(
      view.getByText(
        m.settings_remote_access_strategy_lan_authenticated_warning(),
      ),
    )
    .toBeVisible()
  await expect.element(https).toBeChecked()
  await expect.element(https).toBeDisabled()
})

test('HTTPS can be chosen on its own for the loopback strategies', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    { enabled: true, port: 8765, strategy: 'local_authenticated', tls: 'http' },
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
        strategy: 'local_authenticated',
        tls: 'self_signed',
      },
    },
  })
})

test('the LAN address is validated, saved, and cleared to every interface', async ({
  onTestFinished,
}) => {
  const { view, saved } = await mount(
    onTestFinished,
    <ServerSettings />,
    {},
    lan,
  )
  const address = view.getByRole('textbox', {
    name: m.settings_remote_access_lan_bind_title(),
  })
  const save = view.getByRole('button', { name: m.common_save() }).first()
  await expect.element(address).toBeEnabled()

  await address.fill('300.1.1.1')
  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.settings_remote_access_lan_bind_invalid())
  await expect.element(save).toBeDisabled()

  await address.fill('192.168.1.20')
  await save.click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { ...lan, lan_bind: '192.168.1.20' } },
  })

  await address.fill('fd00::5')
  await expect.element(save).toBeEnabled()

  await address.fill('')
  await save.click()
  expect(saved).toHaveBeenLastCalledWith({
    config: { http: { ...lan, lan_bind: null } },
  })
})

test('other devices are told where to connect while the server listens', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      get_http_server_status: () => ({
        ...on,
        url: 'https://127.0.0.1:12345/',
        tls: { fingerprint, not_after_ms: Date.UTC(2028, 0, 2) },
      }),
    },
    lan,
  )

  await expect
    .element(
      view.getByText(m.settings_remote_access_lan_url_hint({ port: 12345 })),
    )
    .toBeVisible()
})

test('the certificate fingerprint is shown and replacing it asks first', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      get_http_server_status: () => ({
        ...on,
        tls: { fingerprint, not_after_ms: Date.UTC(2028, 0, 2) },
      }),
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

const listening = {
  ...on,
  tls: { fingerprint, not_after_ms: Date.UTC(2028, 0, 2) },
}
const onePort = { ...lan, lan_bind: '192.168.1.20' }

test('an address the certificate does not cover is explained next to the replacement', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      get_http_server_status: () => ({
        ...listening,
        bind_outside_certificate: true,
      }),
    },
    onePort,
  )

  await expect
    .element(
      view.getByText(m.settings_remote_access_certificate_address_mismatch()),
    )
    .toBeVisible()
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_remote_access_certificate_reset(),
      }),
    )
    .toBeEnabled()
})

test('no address warning is shown while the certificate covers the address', async ({
  onTestFinished,
}) => {
  const { view } = await mount(
    onTestFinished,
    <ServerSettings />,
    {
      get_http_server_status: () => ({
        ...listening,
        bind_outside_certificate: false,
      }),
    },
    onePort,
  )

  await expect.element(view.getByText(fingerprint)).toBeVisible()
  expect(
    view
      .getByText(m.settings_remote_access_certificate_address_mismatch())
      .query(),
  ).toBeNull()
})

test('a certificate the listener cannot serve can still be replaced', async ({
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
