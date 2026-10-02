import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useReleaseChannel } from '../src/ipc/use-release-channel'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

async function setup(onTestFinished: TestContext['onTestFinished']) {
  let commit: (value: unknown) => void = () => {}
  const testRpc = createTestRpc({
    get_release_channel: async () => ({
      current: 'stable',
      installed: 'stable',
    }),
    set_release_channel: async () =>
      new Promise((resolve) => {
        commit = resolve
      }),
  })
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const hook = await renderHook(() => useReleaseChannel(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })
  await expect
    .poll(() => hook.result.current.query.data?.current)
    .toBe('stable')
  return { hook, client, testRpc, commit: (value: unknown) => commit(value) }
}

test('channel changes are displayed only after persistence succeeds', async ({
  onTestFinished,
}) => {
  const { hook, testRpc, commit } = await setup(onTestFinished)
  let pending!: Promise<unknown>
  await hook.act(() => {
    pending = hook.result.current.mutation.mutateAsync('beta')
  })
  await expect.poll(() => hook.result.current.mutation.isPending).toBe(true)
  expect(hook.result.current.query.data?.current).toBe('stable')
  await hook.act(async () => {
    commit({
      status: 'committed',
      value: null,
      commits: [],
      notifications_pending: true,
    })
    await pending
  })
  await expect
    .poll(() => hook.result.current.query.data)
    .toEqual({
      current: 'beta',
      installed: 'stable',
    })
  expect(testRpc.invoke).toHaveBeenCalledWith('set_release_channel', {
    channel: 'beta',
  })
})

test('backend rejection keeps the committed nightly channel visible', async ({
  onTestFinished,
}) => {
  const { hook, client, testRpc } = await setup(onTestFinished)
  await hook.act(() => {
    client.setQueryData(['getReleaseChannel'], {
      current: 'nightly',
      installed: 'nightly',
    })
  })
  testRpc.invoke.mockRejectedValue('cannot leave the nightly release channel')
  await hook.act(async () => {
    await expect(
      hook.result.current.mutation.mutateAsync('stable'),
    ).rejects.toBeDefined()
  })
  expect(hook.result.current.query.data?.current).toBe('nightly')
})
