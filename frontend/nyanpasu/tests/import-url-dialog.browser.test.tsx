import { useState } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import type { ImportUrlRequest } from '@/components/profile-import/import-url-dialog'
import {
  ImportUrlProvider,
  useImportUrl,
} from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import type { ProfileUrlProbe } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

type RpcCall = { method: string; params: Record<string, unknown> }

const outcome = (value: unknown) => ({
  status: 'committed',
  value,
  commits: [],
  notifications_pending: false,
})

const LUA = 'function main(config)\n  return config\nend\n'

const luaProbe = {
  classification: {
    suggested: 'lua',
    evidence: [
      { kind: 'lua', signal: 'file_extension' },
      { kind: 'config', signal: 'subscription_quota' },
    ],
  },
  accepted: ['javascript', 'lua'],
  suggested_name: 'rename',
  subscription: { upload: 1, download: 1, total: 4 },
  suggested_update_interval_minutes: 360,
  content: LUA,
} satisfies ProfileUrlProbe

const request = {
  url: 'https://example.com/rename.lua',
  name: null,
  option: null,
} satisfies ImportUrlRequest

/** Probes with `probes` in turn; a probe that is an Error fails. */
function installRpcMock(calls: RpcCall[], probes: (ProfileUrlProbe | Error)[]) {
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const call = args as RpcCall
    calls.push(call)

    switch (call.method) {
      case 'probe_profile_url': {
        const probe = probes.shift()
        if (probe instanceof Error) throw probe
        return probe
      }
      case 'import_profile':
      case 'create_profile':
        return outcome('profile-1')
      default:
        throw new Error(`Unexpected RPC: ${call.method}`)
    }
  })
}

function Importer() {
  const importUrl = useImportUrl()
  const [result, setResult] = useState<string>('pending')

  return (
    <button
      type="button"
      onClick={async () => setResult(String(await importUrl(request)))}
    >
      {`open ${result}`}
    </button>
  )
}

async function mount(onTestFinished: (fn: () => Promise<void>) => void) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider client={queryClient}>
      <BlockTaskProvider>
        <ImportUrlProvider>
          <Importer />
        </ImportUrlProvider>
      </BlockTaskProvider>
    </QueryClientProvider>,
  )

  onTestFinished(async () => {
    await view.unmount()
    queryClient.clear()
    clearMocks()
  })

  await view.getByRole('button', { name: 'open pending' }).click()
  return view
}

const confirm = () => ({ name: m.profile_import_url_confirm() })

test('the suggested kind is preselected, with its reasons, and only accepted kinds are offered', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [luaProbe])
  const view = await mount(onTestFinished)

  await expect
    .element(view.getByRole('radio', { name: 'Lua' }))
    .toHaveAttribute('aria-checked', 'true')
  await expect
    .element(
      view.getByRole('radio', { name: m.profile_import_url_kind_config() }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByRole('radio', { name: m.profile_import_url_kind_overlay() }),
    )
    .toBeDisabled()
  await expect
    .element(view.getByText(m.profile_import_url_detected({ kind: 'Lua' })))
    .toBeVisible()
  // Only the reasons for the suggested kind are listed.
  await expect
    .element(view.getByText(m.profile_import_url_signal_file_extension()))
    .toBeVisible()
  await expect
    .element(view.getByText(m.profile_import_url_signal_subscription_quota()))
    .not.toBeInTheDocument()
  await expect
    .element(view.getByRole('textbox').first())
    .toHaveValue(request.url)
  await expect
    .element(view.getByRole('textbox').first())
    .toHaveAttribute('readonly')
  // Without a server title, the prefilled name comes from the link.
  await expect.element(view.getByRole('textbox').nth(1)).toHaveValue('rename')
  await expect
    .element(view.getByText(m.profile_import_url_name_from_link()))
    .toBeVisible()
  await expect
    .element(
      view.getByText(m.profile_import_url_update_interval({ minutes: 360 })),
    )
    .toBeVisible()

  await view.getByRole('button', confirm()).click()

  await expect
    .element(view.getByRole('button', { name: 'open profile-1' }))
    .toBeVisible()
  expect(calls).toEqual([
    {
      method: 'probe_profile_url',
      params: { url: request.url, option: null },
    },
    {
      method: 'import_profile',
      params: {
        url: request.url,
        name: null,
        option: null,
        transform: { type: 'script', runtime: 'lua' },
        folder: null,
      },
    },
  ])
})

test('a local import creates the downloaded content without downloading again', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [luaProbe])
  const view = await mount(onTestFinished)

  await view.getByRole('radio', { name: m.profile_source_local() }).click()
  await view.getByRole('button', confirm()).click()

  await expect
    .element(view.getByRole('button', { name: 'open profile-1' }))
    .toBeVisible()
  expect(calls.map((call) => call.method)).toEqual([
    'probe_profile_url',
    'create_profile',
  ])
  expect(calls[1].params).toEqual({
    request: {
      metadata: { name: 'rename', desc: null },
      definition: {
        type: 'transform',
        transform: {
          type: 'script',
          runtime: 'lua',
          source: {
            type: 'local',
            binding: { type: 'managed', file: 'pending.yaml' },
          },
        },
      },
      folder: null,
    },
    fileData: LUA,
  })
})

test('a name the user changes is pinned on a remote import', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [luaProbe])
  const view = await mount(onTestFinished)

  await view.getByRole('textbox').nth(1).fill('My rename')
  await view.getByRole('button', confirm()).click()

  await expect
    .element(view.getByRole('button', { name: 'open profile-1' }))
    .toBeVisible()
  expect(calls.at(-1)?.params.name).toBe('My rename')
})

test('without a suggestion the user picks a kind before importing', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [
    {
      ...luaProbe,
      classification: { suggested: null, evidence: [] },
    },
  ])
  const view = await mount(onTestFinished)

  await expect
    .element(view.getByText(m.profile_import_url_no_suggestion()))
    .toBeVisible()
  await expect.element(view.getByRole('button', confirm())).toBeDisabled()

  await view.getByRole('radio', { name: 'JavaScript' }).click()
  await view.getByRole('button', confirm()).click()

  await expect
    .element(view.getByRole('button', { name: 'open profile-1' }))
    .toBeVisible()
  expect(calls.at(-1)?.params.transform).toEqual({
    type: 'script',
    runtime: 'javascript',
  })
})

test('a failed download can be retried', async ({ onTestFinished }) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [new Error('connection refused'), luaProbe])
  const view = await mount(onTestFinished)

  await view.getByRole('button', { name: m.profile_import_url_retry() }).click()

  await expect
    .element(view.getByRole('radio', { name: 'Lua' }))
    .toHaveAttribute('aria-checked', 'true')
  expect(calls.map((call) => call.method)).toEqual([
    'probe_profile_url',
    'probe_profile_url',
  ])
})

test('cancelling resolves without importing', async ({ onTestFinished }) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, [luaProbe])
  const view = await mount(onTestFinished)

  await expect.element(view.getByRole('button', confirm())).toBeVisible()
  await view.getByRole('button', { name: m.common_cancel() }).click()

  await expect
    .element(view.getByRole('button', { name: 'open null' }))
    .toBeVisible()
  expect(calls.map((call) => call.method)).toEqual(['probe_profile_url'])
})
