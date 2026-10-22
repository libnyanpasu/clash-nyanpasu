import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import SubscriptionSection from '@/pages/(main)/main/profiles/$type/detail/_modules/subscription-section'
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

const profileWith = (
  option: Partial<{
    user_agent: string | null
    with_proxy: boolean
    self_proxy: boolean
    update_interval_minutes: number
  }> = {},
) =>
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
          update_interval_minutes: 0,
          ...option,
        },
      },
    },
  }) satisfies ProfileItem_Serialize

// Every patch leaves the fields it does not edit as null, which the backend
// reads as unchanged.
const unchanged = {
  with_proxy: null,
  self_proxy: null,
  update_interval_minutes: null,
}

function installRpcMock(calls: RpcCall[], fail = false) {
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const call = args as RpcCall
    calls.push(call)

    switch (call.method) {
      case 'patch_remote_profile_options':
        if (fail) {
          throw new Error('patch failed')
        }

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

type View = Awaited<ReturnType<typeof mount>>

const row = (view: View, label: string) =>
  view.getByRole('button', { name: label, exact: false })

const switchOf = (view: View, label: string) =>
  view.getByRole('switch', { name: label })

const patchOf = (call: RpcCall) => (call.params as { patch: unknown }).patch

test('the user agent is saved on its own from its dialog', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionSection profile={profileWith()} />,
    onTestFinished,
  )

  await row(view, m.profile_user_agent_label()).click()
  const input = view.getByRole('textbox')
  await input.fill('Custom/1.0')
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  expect(calls[0]).toEqual({
    method: 'patch_remote_profile_options',
    params: {
      uid: 'profile-1',
      patch: { ...unchanged, user_agent: 'Custom/1.0' },
    },
  })
  // No success toast; the dialog just closes.
  expect(notifications.message).not.toHaveBeenCalled()
  await expect.poll(() => view.getByRole('textbox').elements()).toHaveLength(0)
})

test('an empty user agent clears the override', async ({ onTestFinished }) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionSection profile={profileWith({ user_agent: 'Existing/2' })} />,
    onTestFinished,
  )

  await row(view, m.profile_user_agent_label()).click()
  const input = view.getByRole('textbox')
  await expect.element(input).toHaveValue('Existing/2')
  await input.clear()
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  expect(patchOf(calls[0])).toEqual({ ...unchanged, user_agent: null })
})

test('the update interval is saved on its own from its dialog', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 90 })}
    />,
    onTestFinished,
  )

  await row(view, m.profile_update_interval_label()).click()
  const input = view.getByRole('textbox')
  await expect.element(input).toHaveValue('90')
  await input.fill('45')
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => calls.length).toBe(1)
  expect(calls[0]).toEqual({
    method: 'patch_remote_profile_options',
    params: {
      uid: 'profile-1',
      patch: { ...unchanged, update_interval_minutes: 45 },
    },
  })
})

test('the update interval cannot be left empty', async ({ onTestFinished }) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 90 })}
    />,
    onTestFinished,
  )

  await row(view, m.profile_update_interval_label()).click()
  // The field clamps typed values to its minimum, so only an empty one is invalid.
  await view.getByRole('textbox').clear()
  await view.getByRole('button', { name: m.common_save() }).click()

  await expect
    .element(view.getByText(m.profile_form_option_update_interval_min_error()))
    .toBeVisible()
  expect(calls).toEqual([])
})

test('the update interval row shows only while auto update is on', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const off = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 0 })}
    />,
    onTestFinished,
  )

  await expect
    .element(switchOf(off, m.profile_update_option_auto_update_label()))
    .not.toBeChecked()
  expect(row(off, m.profile_update_interval_label()).elements()).toHaveLength(0)

  await off.unmount()
  const on = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 60 })}
    />,
    onTestFinished,
  )

  await expect
    .element(switchOf(on, m.profile_update_option_auto_update_label()))
    .toBeChecked()
  await expect.element(row(on, m.profile_update_interval_label())).toBeVisible()
  expect(calls).toEqual([])
})

test('auto update switches on with the default interval and off with zero', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const off = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 0 })}
    />,
    onTestFinished,
  )

  await switchOf(off, m.profile_update_option_auto_update_label()).click()
  await expect.poll(() => calls.length).toBe(1)
  expect(patchOf(calls[0])).toEqual({
    ...unchanged,
    update_interval_minutes: 120,
  })

  await off.unmount()
  const on = await mount(
    <SubscriptionSection
      profile={profileWith({ update_interval_minutes: 90 })}
    />,
    onTestFinished,
  )

  await switchOf(on, m.profile_update_option_auto_update_label()).click()
  await expect.poll(() => calls.length).toBe(2)
  expect(patchOf(calls[1])).toEqual({
    ...unchanged,
    update_interval_minutes: 0,
  })
})

test('each proxy switch sends only its own field', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  installRpcMock(calls)
  const view = await mount(
    <SubscriptionSection profile={profileWith()} />,
    onTestFinished,
  )

  await switchOf(view, m.profile_with_proxy_label()).click()
  await expect.poll(() => calls.length).toBe(1)
  expect(patchOf(calls[0])).toEqual({ ...unchanged, with_proxy: true })

  await switchOf(view, m.profile_self_proxy_label()).click()
  await expect.poll(() => calls.length).toBe(2)
  expect(patchOf(calls[1])).toEqual({ ...unchanged, self_proxy: true })
})

test('a failed patch is reported as an error', async ({ onTestFinished }) => {
  const calls: RpcCall[] = []
  installRpcMock(calls, true)
  const view = await mount(
    <SubscriptionSection profile={profileWith()} />,
    onTestFinished,
  )

  await switchOf(view, m.profile_with_proxy_label()).click()

  await expect.poll(() => notifications.message.mock.calls.length).toBe(1)
  expect(notifications.message.mock.calls[0][1]).toMatchObject({
    kind: 'error',
  })
})
