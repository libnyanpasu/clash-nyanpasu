import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { Proxies_Serialize, ProxyGroup } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { useClashProxies } from '../src/ipc/use-clash-proxies'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const pinned: ProxyGroup = {
  name: 'auto',
  type: 'URLTest',
  all: ['a'],
  now: 'a',
  fixed: 'a',
  testUrl: null,
  expectedStatus: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: true },
}

test('clearing a pin calls the mutation, then refetches the proxies', async ({
  onTestFinished,
}) => {
  let snapshot: Proxies_Serialize = {
    global: null,
    groups: [pinned],
    nodes: {},
  }
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => snapshot,
    clear_proxy_fixed: async () => {
      snapshot = { ...snapshot, groups: [{ ...pinned, fixed: null }] }
      return {
        status: 'committed',
        value: null,
        commits: [],
        notifications_pending: false,
      }
    },
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)

  await hook.act(async () => {
    await hook.result.current.clearProxyFixed('auto')
  })

  expect(testRpc.invoke).toHaveBeenCalledWith('clear_proxy_fixed', {
    group: 'auto',
  })
  await expect
    .poll(() => hook.result.current.proxies.data?.groups[0].fixed)
    .toBeNull()
})
