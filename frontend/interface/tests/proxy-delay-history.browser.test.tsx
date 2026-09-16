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
  all: [node('tested'), node('other')],
})

async function setup(
  response: Record<string, number>,
  onTestFinished: TestContext['onTestFinished'],
) {
  const snapshot: Proxies_Serialize = {
    global: group('GLOBAL'),
    groups: [group('group')],
    direct: node('DIRECT'),
    proxies: [],
    records: {},
  }
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

test('group delay preserves history of nodes absent from the response', async ({
  onTestFinished,
}) => {
  const { original, data } = await setup({ tested: 100 }, onTestFinished)
  for (const group of [data().global, ...data().groups]) {
    expect(group.all[0].history.map(({ delay }) => delay)).toEqual([42, 100])
    expect(group.all[1].history).toEqual(original.global.all[1].history)
  }
  for (const group of [original.global, ...original.groups]) {
    expect(group.all[0].history).toHaveLength(1)
  }
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
  expect(data().groups[0].all[0].history.map(({ delay }) => delay)).toEqual([
    42, 0,
  ])
})
