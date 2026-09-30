import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  queries,
  type Proxies_Serialize,
  type ProxyGroupItem_Serialize,
  type ProxyItem_Serialize,
} from '../src/ipc/bindings'
import {
  useClashProxies,
  type ClashProxiesQuery,
} from '../src/ipc/use-clash-proxies'

const ipc = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
}))

vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  invoke: ipc.invoke,
}))

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
  ipc.invoke.mockImplementation(async (command) => {
    if (command === 'get_proxies') return snapshot
    if (command === 'clash_api_get_group_delay') return response
    throw new Error(`Unexpected IPC command: ${command}`)
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
    ipc.invoke.mockReset()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = queries.getProxies().queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()
  // Freeze only polling: React Query notifications and browser assertions keep
  // real time, while a slow CI worker cannot refetch stale fixture data.
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  await hook.act(async () => {
    await hook.result.current.updateGroupDelay.mutateAsync(['group'])
  })
  expect(ipc.invoke).toHaveBeenCalledWith('clash_api_get_group_delay', {
    group: 'group',
    url: null,
  })
  expect(vi.getTimerCount()).toBe(0)
  // A polling refetch must not silently overwrite the response being asserted.
  expect(
    ipc.invoke.mock.calls.filter(([command]) => command === 'get_proxies'),
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
  ipc.invoke.mockImplementation(async (command) => {
    if (command === 'get_proxies') return snapshot
    if (command === 'clash_api_get_proxy_delay') return { delay: 100 }
    throw new Error(`Unexpected IPC command: ${command}`)
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
    ipc.invoke.mockReset()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = queries.getProxies().queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()

  await hook.act(async () => {
    await hook.result.current.updateProxiesDelay.mutateAsync(['tested', null])
  })

  expect(ipc.invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'tested',
    provider: null,
    url: null,
  })
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  expect(data().nodes.other).toBe(original.nodes.other)
})
