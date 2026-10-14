import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import UpdateOptionEditor from '@/pages/(main)/main/profiles/$type/detail/_modules/update-option-editor'
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

const profileWith = (updateIntervalMinutes: number) =>
  ({
    uid: 'profile-1',
    name: 'Remote subscription',
    type: 'config',
    config: {
      type: 'file',
      transforms: [],
      source: {
        type: 'remote',
        file: 'profile.yaml',
        url: 'https://example.com/subscription',
        option: {
          user_agent: null,
          with_proxy: false,
          self_proxy: false,
          update_interval_minutes: updateIntervalMinutes,
        },
      },
    },
  }) satisfies ProfileItem_Serialize

function installRpcMock(calls: RpcCall[]) {
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const call = args as RpcCall
    calls.push(call)

    switch (call.method) {
      case 'patch_remote_profile_options':
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

// The user agent field is the first textbox; the interval is the second one
// and is only rendered while auto update is on.
const intervalInput = (view: Awaited<ReturnType<typeof mount>>) =>
  view.getByRole('textbox').nth(1)

const expectIntervalHidden = (view: Awaited<ReturnType<typeof mount>>) =>
  expect.poll(() => view.getByRole('textbox').elements().length).toBe(1)

test('update option editor turns auto update off by saving a zero interval', async ({
  onTestFinished,
}) => {
  const interval = 90
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <UpdateOptionEditor profile={profileWith(interval)}>
      <span>Update opts</span>
    </UpdateOptionEditor>,
    onTestFinished,
  )

  await view.getByRole('button', { name: 'Update opts' }).click()
  const autoUpdate = view.getByRole('switch').first()
  await expect.element(autoUpdate).toBeChecked()
  await expect.element(intervalInput(view)).toHaveValue(String(interval))

  await autoUpdate.click()
  await expect.element(autoUpdate).not.toBeChecked()
  await expectIntervalHidden(view)
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  expect(calls[0]).toEqual({
    method: 'patch_remote_profile_options',
    params: {
      uid: 'profile-1',
      patch: {
        user_agent: null,
        with_proxy: false,
        self_proxy: false,
        update_interval_minutes: 0,
      },
    },
  })
})

test('update option editor hides the interval while auto update is off and saves the default when switched on', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <UpdateOptionEditor profile={profileWith(0)}>
      <span>Update opts</span>
    </UpdateOptionEditor>,
    onTestFinished,
  )

  await view.getByRole('button', { name: 'Update opts' }).click()
  const autoUpdate = view.getByRole('switch').first()
  await expect.element(autoUpdate).not.toBeChecked()
  await expectIntervalHidden(view)

  await autoUpdate.click()
  await expect.element(autoUpdate).toBeChecked()
  await expect.element(intervalInput(view)).toHaveValue('')
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  const { patch } = calls[0].params as {
    patch: { update_interval_minutes: number }
  }
  expect(patch.update_interval_minutes).toBe(120)
})

test('update option editor saves the default interval when the field is cleared', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <UpdateOptionEditor profile={profileWith(90)}>
      <span>Update opts</span>
    </UpdateOptionEditor>,
    onTestFinished,
  )

  await view.getByRole('button', { name: 'Update opts' }).click()
  await intervalInput(view).clear()
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  const { patch } = calls[0].params as {
    patch: { update_interval_minutes: number }
  }
  expect(patch.update_interval_minutes).toBe(120)
})
