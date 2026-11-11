import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { RpcProvider } from '@nyanpasu/query'
import {
  createIconTransport,
  createRpcClient,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import type { ConfigurationStatus, MutationOutcome } from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import ConfigurationStatusPage from '../src/pages/(main)/main/settings/debug/_modules/configuration-status'
import HotkeyManager from '../src/pages/(main)/main/settings/nyanpasu/shortcuts/_modules/hotket-manager'
import SystemServiceSwitch from '../src/pages/(main)/main/settings/system/service/_modules/system-service-switch'

const platform = vi.hoisted(() => ({ desktop: true }))
const debug = vi.hoisted(() => ({ status: vi.fn(), retryEffect: vi.fn() }))

vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => platform.desktop,
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({
  commands: {
    retryConfigurationEffect: debug.retryEffect,
  },
  queries: {
    getConfigurationStatus: () => ({
      queryKey: ['getConfigurationStatus'],
      queryFn: debug.status,
    }),
  },
  events: { configurationStatusChanged: { listen: async () => () => {} } },
}))

const outcome: MutationOutcome<null> = {
  status: 'committed_degraded',
  value: null,
  commits: [],
  notifications_pending: true,
  degradations: [],
}

const status: ConfigurationStatus = {
  event_seq: 1,
  maintenance: null,
  active: null,
  recent_operations: [],
  sources: [],
  source_versions: { application: 1, clash: 1, session: 1, profiles: 1 },
  runtime: {
    health: 'healthy',
    operation_id: null,
    attempts: 0,
    automatic_remaining: 0,
    message: null,
  },
  effects: [
    {
      kind: 'hotkeys',
      health: 'blocked',
      desired_revision: 1,
      applied_revision: 0,
      attempts: 1,
      automatic_remaining: 0,
      code: 'hotkey_partial_registration',
      message: null,
    },
  ],
}

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  desktop: boolean,
  initialEnabled = false,
) {
  platform.desktop = desktop
  let enabled = initialEnabled
  const invoke = vi.fn(
    async (method: string, params?: Record<string, unknown>) => {
      switch (method) {
        case 'get_app_config':
          return { enable_service_mode: enabled }
        case 'status_service':
          return {
            name: 'test-service',
            version: '2.0.2',
            status: 'running',
            server: null,
            compat: { kind: 'compatible' },
            phase: 'ready',
            restart_attempts: 0,
          }
        case 'get_core_status':
          return {
            controller: null,
            host: 'local',
            connectivity: { kind: 'connected' },
            generation: 1,
            state: { Running: { epoch: 1, pid: 7 } },
            state_changed_at: 0,
            revision: null,
            healthy: true,
          }
        case 'get_hotkey_functions':
          return ['toggle_tun_mode']
        case 'get_hotkeys':
          return ['toggle_tun_mode,Control+Q']
        case 'set_service_mode':
          enabled = params?.enabled as boolean
          return outcome
        default:
          throw new Error(`Unexpected RPC method: ${method}`)
      }
    },
  )
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  const rpc = createRpcClient({
    icons: createIconTransport(false),
    commands: {
      invoke: async <T,>(method: string, params?: Record<string, unknown>) =>
        (await invoke(method, params)) as T,
    },
    events,
  })
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const view = await render(
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>
        <TooltipProvider>
          <SystemServiceSwitch />
          <HotkeyManager />
        </TooltipProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    rpc.dispose()
    vi.clearAllMocks()
  })
  await expect.element(view.getByText('Q', { exact: true })).toBeVisible()
  return { view, invoke }
}

test('service switch calls set service mode', async ({ onTestFinished }) => {
  const { view, invoke } = await setup(onTestFinished, true)
  const toggle = view.getByRole('switch')
  await expect.element(toggle).toBeEnabled()
  await toggle.click()
  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('set_service_mode', { enabled: true })
  await expect.element(toggle).toBeChecked()
  expect(
    invoke.mock.calls.some(([method]) => method === 'patch_app_config'),
  ).toBe(false)
})

test('browser cannot edit dedicated settings', async ({ onTestFinished }) => {
  const { view, invoke } = await setup(onTestFinished, false, true)
  await expect.element(view.getByRole('switch')).toBeDisabled()
  await expect.element(view.getByRole('switch')).toBeChecked()
  await expect
    .element(view.getByText(m.error_config_hotkey_capability_unsupported()))
    .toBeVisible()
  const editor = document.querySelector('[data-slot="hotkey-manager"]')!
  const controls = editor.querySelectorAll<
    HTMLButtonElement | HTMLInputElement
  >('button, input')
  expect(controls.length).toBe(3)
  for (const control of controls) expect(control.disabled).toBe(true)
  expect(
    invoke.mock.calls.some(([method]) =>
      ['patch_app_config', 'set_service_mode', 'set_hotkeys'].includes(method),
    ),
  ).toBe(false)
})

test('browser can retry saved hotkeys', async ({ onTestFinished }) => {
  platform.desktop = false
  debug.status.mockResolvedValue(status)
  debug.retryEffect.mockResolvedValue({ status: 'ok', data: null })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider client={client}>
      <ConfigurationStatusPage />
    </QueryClientProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    vi.clearAllMocks()
  })
  await expect.element(view.getByText(m.configuration_hotkeys())).toBeVisible()
  const retry = view.getByRole('button', { name: m.configuration_retry() })
  await expect.element(retry).toBeEnabled()
  await retry.click()
  expect(debug.retryEffect).toHaveBeenCalledWith('hotkeys')
})
