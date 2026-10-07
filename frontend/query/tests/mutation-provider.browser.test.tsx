import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useClashProxies } from '../src/ipc/use-clash-proxies'
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

  await testRpc.emitMutation('nyanpasuConfig')

  await expect.poll(fetches).toBe(2)
})
