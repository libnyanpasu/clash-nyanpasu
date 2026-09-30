import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { rpc } from '../src/ipc/rpc'
import {
  type Proxies_Serialize,
  type ProxyGroupItem_Serialize,
  type ProxyItem_Serialize,
} from '../src/ipc/rpc-bindings'
import {
  useClashProxies,
  type ClashProxiesQuery,
} from '../src/ipc/use-clash-proxies'

const fetchRpc =
  vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>()
vi.stubGlobal('fetch', fetchRpc)

const node = (name: string): ProxyItem_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  all: null,
  now: null,
  provider: null,
  alive: null,
  hidden: false,
  history: [{ time: '2026-09-10T00:00:00Z', delay: 42 }],
})

const group = (name: string): ProxyGroupItem_Serialize => ({
  ...node(name),
  type: 'Selector',
  now: 'tested',
  all: ['tested', 'other'],
})

function makeSnapshot(): Proxies_Serialize {
  return {
    global: group('GLOBAL'),
    groups: [group('group')],
    nodes: { tested: node('tested'), other: node('other') },
  }
}

async function setup(
  response: Record<string, number>,
  onTestFinished: TestContext['onTestFinished'],
) {
  const snapshot = makeSnapshot()
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  fetchRpc.mockImplementation(async (_input, init) => {
    const { method } = JSON.parse(String(init?.body)) as { method: string }
    if (method === 'get_proxies')
      return { ok: true, json: async () => snapshot } as Response
    if (method === 'clash_api_get_group_delay')
      return { ok: true, json: async () => response } as Response
    throw new Error(`Unexpected RPC command: ${method}`)
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    vi.clearAllTimers()
    vi.useRealTimers()
    fetchRpc.mockReset()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = rpc.queries.getProxies().queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()
  // Freeze only polling: React Query notifications and browser assertions keep
  // real time, while a slow CI worker cannot refetch stale fixture data.
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  await hook.act(async () => {
    await hook.result.current.updateGroupDelay.mutateAsync(['group'])
  })
  expect(fetchRpc).toHaveBeenCalledWith(
    '/bridge/rpc',
    expect.objectContaining({
      body: JSON.stringify({
        method: 'clash_api_get_group_delay',
        params: { group: 'group', url: null },
      }),
    }),
  )
  expect(vi.getTimerCount()).toBe(0)
  // A polling refetch must not silently overwrite the response being asserted.
  expect(
    fetchRpc.mock.calls.filter(
      ([, init]) => JSON.parse(String(init?.body)).method === 'get_proxies',
    ),
  ).toHaveLength(1)
  return { original, data }
}

test('group delay replaces only the tested node, keeping the sibling identity', async ({
  onTestFinished,
}) => {
  const { original, data } = await setup({ tested: 100 }, onTestFinished)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  // Only one `tested` object backs both `global.all` and `groups[0].all`, so
  // it is replaced once, and the untouched sibling keeps its identity.
  expect(data().nodes.other).toBe(original.nodes.other)
  expect(original.nodes.tested.history).toHaveLength(1)
})

test('empty group responses retain all existing history', async ({
  onTestFinished,
}) => {
  const { original, data } = await setup({}, onTestFinished)
  expect(data()).toEqual(original)
})

test('zero delay is retained as a failed sample', async ({
  onTestFinished,
}) => {
  const { data } = await setup({ tested: 0 }, onTestFinished)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([42, 0])
})

test('single node delay only replaces that node, keeping sibling identity', async ({
  onTestFinished,
}) => {
  const snapshot = makeSnapshot()
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  fetchRpc.mockImplementation(async (_input, init) => {
    const { method } = JSON.parse(String(init?.body)) as { method: string }
    if (method === 'get_proxies')
      return { ok: true, json: async () => snapshot } as Response
    if (method === 'clash_api_get_proxy_delay')
      return { ok: true, json: async () => ({ delay: 100 }) } as Response
    throw new Error(`Unexpected IPC command: ${method}`)
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    fetchRpc.mockReset()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = rpc.queries.getProxies().queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()

  await hook.act(async () => {
    await hook.result.current.updateProxiesDelay.mutateAsync(['tested', null])
  })

  expect(fetchRpc).toHaveBeenCalledWith(
    '/bridge/rpc',
    expect.objectContaining({
      body: JSON.stringify({
        method: 'clash_api_get_proxy_delay',
        params: { name: 'tested', provider: null, url: null },
      }),
    }),
  )
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  expect(data().nodes.other).toBe(original.nodes.other)
})
