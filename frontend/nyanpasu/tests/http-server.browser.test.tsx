import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import { QueryClient } from '@tanstack/react-query'
import { createTestRpc, rpcWrapper } from '../../query/tests/rpc-test-utils'
import HttpServer from '../src/pages/(main)/main/settings/debug/_modules/http-server'

const backend = vi.hoisted(() => ({ desktop: true }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => backend.desktop,
}))

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

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  overrides: Record<string, () => unknown> = {},
  desktop = true,
) {
  backend.desktop = desktop
  const state = { status: off as typeof off | typeof on }
  const savedRemoteAccess = vi.fn()
  const testRpc = createTestRpc({
    get_http_server_status: () => state.status,
    get_app_config: () => ({
      remote_access: { http: { enabled: state.status.enabled, port: null } },
    }),
    set_remote_access: (params) => {
      savedRemoteAccess(params)
      state.status = (params?.config as { http: { enabled: boolean } }).http
        .enabled
        ? on
        : off
      return committed
    },
    create_http_launch_url: () => 'http://127.0.0.1:12345/?code=once',
    open_web_url: () => null,
    ...overrides,
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const view = await render(
    <Wrapper>
      <HttpServer />
    </Wrapper>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllMocks()
  })
  const toggle = view.getByRole('switch', {
    name: m.settings_debug_http_server_title(),
  })
  if (desktop) await expect.element(toggle).toBeEnabled()
  else await expect.element(toggle).toBeDisabled()
  return { view, toggle, testRpc, savedRemoteAccess }
}

test('the switch writes the whole remote access section and shows the origin', async ({
  onTestFinished,
}) => {
  const { view, toggle, savedRemoteAccess } = await setup(onTestFinished)

  await toggle.click()

  await expect.element(toggle).toBeChecked()
  await expect
    .element(
      view.getByText(
        m.settings_debug_http_server_listening({
          url: 'http://127.0.0.1:12345/',
        }),
      ),
    )
    .toBeVisible()
  expect(savedRemoteAccess).toHaveBeenLastCalledWith({
    config: { http: { enabled: true, port: null } },
  })

  await toggle.click()
  await expect.element(toggle).not.toBeChecked()
  expect(savedRemoteAccess).toHaveBeenLastCalledWith({
    config: { http: { enabled: false, port: null } },
  })
})

test('the launch URL is requested on demand and handed to the opener only', async ({
  onTestFinished,
}) => {
  const { view, toggle, testRpc } = await setup(onTestFinished)
  await toggle.click()
  await expect.element(toggle).toBeChecked()
  expect(testRpc.invoke).not.toHaveBeenCalledWith(
    'create_http_launch_url',
    expect.anything(),
  )

  await view
    .getByRole('button', {
      name: m.settings_debug_http_server_open(),
    })
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
  const { view, toggle, savedRemoteAccess } = await setup(onTestFinished, {
    get_http_server_status: () => ({ ...off, fault: 'port_in_use' }),
    get_app_config: () => ({
      remote_access: { http: { enabled: true, port: 8765 } },
    }),
  })

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(m.settings_debug_http_server_fault_port_in_use())
  // The committed desire is on although nothing listens, and nothing is rolled back.
  await expect.element(toggle).toBeChecked()

  await toggle.click()

  expect(savedRemoteAccess).toHaveBeenLastCalledWith({
    config: { http: { enabled: false, port: 8765 } },
  })
})

test('a failed change reports the error', async ({ onTestFinished }) => {
  const { view, toggle } = await setup(onTestFinished, {
    set_remote_access: () => {
      throw new Error('commit failed')
    },
  })

  await toggle.click()

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent(
      'The operation result is unconfirmed. Inspect its current status before trying again.',
    )
})

test('browser can inspect status but cannot stop its own transport', async ({
  onTestFinished,
}) => {
  const { toggle, savedRemoteAccess } = await setup(onTestFinished, {}, false)
  await expect.element(toggle).toBeDisabled()
  expect(savedRemoteAccess).not.toHaveBeenCalled()
})
