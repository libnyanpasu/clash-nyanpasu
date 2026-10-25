import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useClashProxies } from '../src/ipc/use-clash-proxies'
import { useProfileContent } from '../src/ipc/use-profile-content'
import { MutationProvider } from '../src/provider/mutation-provider'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

test('an app config change refetches the proxies snapshot, which is trimmed to the default test URL', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
    },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => ({ global: null, groups: [], nodes: {} }),
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: ({ children }) => (
      <Wrapper>
        <MutationProvider>{children}</MutationProvider>
      </Wrapper>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const fetches = () =>
    testRpc.invoke.mock.calls.filter(([method]) => method === 'get_proxies')
      .length
  expect(fetches()).toBe(1)

  await testRpc.emitStateChanged({ kind: 'nyanpasu_config' })

  await expect.poll(fetches).toBe(2)
})

test('a profiles change refetches only the files it names', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
    },
  })
  const testRpc = createTestRpc({
    read_profile_file: async (params) => `uid: ${params?.uid}`,
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook(
    () => [useProfileContent('a').query, useProfileContent('b').query],
    {
      wrapper: ({ children }) => (
        <Wrapper>
          <MutationProvider>{children}</MutationProvider>
        </Wrapper>
      ),
    },
  )
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect
    .poll(() => hook.result.current.every((query) => query.isSuccess))
    .toBe(true)
  const reads = (uid: string) =>
    testRpc.invoke.mock.calls.filter(
      ([method, params]) =>
        method === 'read_profile_file' && params?.uid === uid,
    ).length

  await testRpc.emitStateChanged({ kind: 'profiles', files: ['a'] })

  await expect.poll(() => reads('a')).toBe(2)
  expect(reads('b')).toBe(1)
})
