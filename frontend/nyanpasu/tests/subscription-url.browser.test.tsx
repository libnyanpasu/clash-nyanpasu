import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProfileQuickImport from '@/pages/(main)/main/profiles/_modules/profile-quick-import'
import CreateProfileModal from '@/pages/(main)/main/profiles/$type/_modules/create-profile-modal'
import SubscriptionUrlEditor from '@/pages/(main)/main/profiles/$type/detail/_modules/subscription-url-editor'
import { m } from '@/paraglide/messages'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

const notifications = vi.hoisted(() => ({ message: vi.fn() }))
vi.mock('@/utils/notification', () => notifications)

type RpcCall = { method: string; params: Record<string, unknown> }

const outcome = (value: unknown) => ({
  status: 'committed',
  value,
  commits: [],
  notifications_pending: false,
})

const loopbackUrls = [
  'http://127.0.0.1:9090/subscription?token=abc%2F123',
  'https://[::1]:9443/profile?token=abc%2F123',
  'http://localhost:9090/profile?token=abc%2F123',
]

const profile = {
  uid: 'profile-1',
  name: 'Local subscription',
  type: 'config',
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'remote',
      file: 'profile.yaml',
      url: 'https://example.com/subscription?token=old',
      option: {
        user_agent: null,
        with_proxy: false,
        self_proxy: false,
        update_interval_minutes: 0,
      },
    },
  },
} satisfies ProfileItem_Serialize

function installRpcMock(calls: RpcCall[]) {
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const call = args as RpcCall
    calls.push(call)

    switch (call.method) {
      case 'import_profile':
        return outcome('profile-1')
      case 'replace_profile_definition':
      case 'update_profile':
        return outcome(null)
      default:
        throw new Error(`Unexpected RPC: ${call.method}`)
    }
  })
}

async function mount(
  element: ReactNode,
  onTestFinished: (fn: () => void | Promise<void>) => void,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider client={queryClient}>
      <BlockTaskProvider>{element}</BlockTaskProvider>
    </QueryClientProvider>,
  )

  onTestFinished(async () => {
    await view.unmount()
    queryClient.clear()
    clearMocks()
    notifications.message.mockReset()
  })

  return view
}

test('quick import submits legal loopback URLs unchanged and rejects invalid schemes', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(<ProfileQuickImport />, onTestFinished)
  const input = view.getByPlaceholder(m.profile_quick_import_placeholder())

  for (const url of loopbackUrls) {
    await input.fill(url)
    await view.getByRole('button').nth(1).click()
    await expect.poll(() => calls.length).toBe(loopbackUrls.indexOf(url) + 1)
    expect(calls.at(-1)).toEqual({
      method: 'import_profile',
      params: { url, name: null, option: null, transform: null },
    })
  }

  notifications.message.mockClear()
  await input.fill('ftp://127.0.0.1:9090/profile')
  await view.getByRole('button').nth(1).click()
  await expect.poll(() => notifications.message.mock.calls.length).toBe(1)
  expect(calls).toHaveLength(loopbackUrls.length)
})

test('remote profile creation submits legal loopback URLs unchanged', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <CreateProfileModal
      open
      onOpenChange={() => {}}
      kind="file"
      source="remote"
    />,
    onTestFinished,
  )
  const input = view.getByRole('textbox').nth(2)

  for (const url of loopbackUrls) {
    await input.fill(url)
    await view.getByRole('button', { name: m.common_submit() }).click()
    await expect.poll(() => calls.length).toBe(loopbackUrls.indexOf(url) + 1)
    expect(calls.at(-1)).toEqual({
      method: 'import_profile',
      params: {
        url,
        name: null,
        option: {
          user_agent: null,
          with_proxy: false,
          self_proxy: false,
          update_interval_minutes: null,
        },
        transform: null,
      },
    })
  }

  await input.fill('127.0.0.1:9090/profile')
  await view.getByRole('button', { name: m.common_submit() }).click()
  await expect.element(view.getByText('Invalid URL')).toBeVisible()
  expect(calls).toHaveLength(loopbackUrls.length)
})

test('subscription URL editor preserves the URL through replace and refresh RPCs', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionUrlEditor profile={profile}>
      <span>Edit subscription</span>
    </SubscriptionUrlEditor>,
    onTestFinished,
  )

  await view.getByRole('button', { name: 'Edit subscription' }).click()
  const input = view.getByRole('textbox').first()
  await input.fill(loopbackUrls[1])
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(2)
  expect(calls[0]).toEqual({
    method: 'replace_profile_definition',
    params: {
      uid: 'profile-1',
      definition: {
        type: 'config',
        config: {
          type: 'file',
          transforms: [],
          source: {
            ...profile.config.source,
            url: loopbackUrls[1],
          },
        },
      },
    },
  })
  expect(calls[1]).toEqual({
    method: 'update_profile',
    params: { uid: 'profile-1', option: null },
  })
})

test('subscription URL editor blocks invalid URLs and resets unsaved input on reopen', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionUrlEditor profile={profile}>
      <span>Edit subscription</span>
    </SubscriptionUrlEditor>,
    onTestFinished,
  )

  await view.getByRole('button', { name: 'Edit subscription' }).click()
  const input = view.getByRole('textbox').first()
  await input.fill('ftp://127.0.0.1:9090/profile')
  await view.getByRole('button', { name: m.common_save() }).click()
  await expect.element(view.getByText('Invalid URL')).toBeVisible()
  await expect.element(input).toHaveValue('ftp://127.0.0.1:9090/profile')
  expect(calls).toHaveLength(0)

  await view.getByRole('button', { name: m.common_cancel() }).click()
  await view.getByRole('button', { name: 'Edit subscription' }).click()
  await expect
    .element(view.getByRole('textbox').first())
    .toHaveValue(profile.config.source.url)
})
