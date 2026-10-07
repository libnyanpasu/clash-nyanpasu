import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import {
  type Proxies_Serialize,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  useClashProxies,
  type ClashProxiesQuery,
} from '../src/ipc/use-clash-proxies'
import { createQueryBindings } from '../src/query-bindings'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const node = (name: string): Proxy_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  history: [{ time: '2026-09-10T00:00:00Z', delay: 42 }],
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})

const group = (name: string): ProxyGroup => ({
  name,
  type: 'Selector',
  all: ['tested', 'other'],
  now: 'tested',
  fixed: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: false },
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
  const testRpc = createTestRpc({
    get_proxies: async () => snapshot,
    mutate_proxies: async () => snapshot,
    clash_api_get_group_delay: async () => response,
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllTimers()
    vi.useRealTimers()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = createQueryBindings(testRpc.rpc).queries.getProxies()
    .queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()
  // Freeze only polling: React Query notifications and browser assertions keep
  // real time, while a slow CI worker cannot refetch stale fixture data.
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  await hook.act(async () => {
    await hook.result.current.updateGroupDelay.mutateAsync(['group'])
  })
  expect(testRpc.invoke).toHaveBeenCalledWith('clash_api_get_group_delay', {
    group: 'group',
    url: null,
  })
  expect(vi.getTimerCount()).toBe(0)
  // A polling refetch must not silently overwrite the response being asserted.
  expect(
    testRpc.invoke.mock.calls.filter(([method]) => method === 'get_proxies'),
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
  const testRpc = createTestRpc({
    get_proxies: async () => snapshot,
    clash_api_get_proxy_delay: async () => ({ delay: 100 }),
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
  const queryKey = createQueryBindings(testRpc.rpc).queries.getProxies()
    .queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  const original = data()

  await hook.act(async () => {
    await hook.result.current.updateProxiesDelay.mutateAsync(['tested', null])
  })

  expect(testRpc.invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'tested',
    provider: null,
    url: null,
  })
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  expect(data().nodes.other).toBe(original.nodes.other)
})

function makePinnedSnapshot(): Proxies_Serialize {
  return {
    global: group('GLOBAL'),
    groups: [
      {
        ...group('group'),
        type: 'URLTest',
        fixed: 'tested',
        capabilities: { select: true, clearFixed: true },
      },
    ],
    nodes: {
      tested: node('tested'),
      other: { ...node('other'), provider: 'sub' },
    },
  }
}

// Renders the hook against a cached snapshot while the core answers
// `mutate_proxies` with `fresh`, the way it does after an external change.
async function renderGroupTest(
  {
    cached,
    fresh,
    delay,
  }: {
    cached: Proxies_Serialize
    fresh: Proxies_Serialize | Error
    delay: (params?: Record<string, unknown>) => Promise<unknown>
  },
  onTestFinished: TestContext['onTestFinished'],
) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => cached,
    mutate_proxies: async () => {
      if (fresh instanceof Error) throw fresh
      return fresh
    },
    clash_api_get_group_delay: async () => ({}),
    clash_api_get_proxy_delay: delay,
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllTimers()
    vi.useRealTimers()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = createQueryBindings(testRpc.rpc).queries.getProxies()
    .queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  return {
    data,
    invoke: testRpc.invoke,
    hook,
    testGroup: () =>
      hook.act(async () => {
        await hook.result.current.updateGroupDelay.mutateAsync(['group'])
      }),
  }
}

async function testPinnedGroup(
  delay: (params?: Record<string, unknown>) => Promise<unknown>,
  onTestFinished: TestContext['onTestFinished'],
) {
  const snapshot = makePinnedSnapshot()
  const { data, invoke, testGroup } = await renderGroupTest(
    { cached: snapshot, fresh: snapshot, delay },
    onTestFinished,
  )
  await testGroup()
  return { data, invoke }
}

const groupDelayCalls = (invoke: { mock: { calls: unknown[][] } }) =>
  invoke.mock.calls.filter(([method]) => method === 'clash_api_get_group_delay')

test('a pinned group tests its members one by one and never the group', async ({
  onTestFinished,
}) => {
  const { data, invoke } = await testPinnedGroup(
    async () => ({ delay: 100 }),
    onTestFinished,
  )
  expect(invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'tested',
    provider: null,
    url: null,
  })
  expect(invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'other',
    provider: 'sub',
    url: null,
  })
  expect(groupDelayCalls(invoke)).toHaveLength(0)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  expect(data().nodes.other.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
})

test('a pin newer than the cached snapshot still tests members one by one', async ({
  onTestFinished,
}) => {
  const { invoke, testGroup } = await renderGroupTest(
    {
      cached: makeSnapshot(),
      fresh: makePinnedSnapshot(),
      delay: async () => ({ delay: 100 }),
    },
    onTestFinished,
  )
  await testGroup()
  expect(invoke).toHaveBeenCalledWith('mutate_proxies', undefined)
  expect(invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'other',
    provider: 'sub',
    url: null,
  })
  expect(groupDelayCalls(invoke)).toHaveLength(0)
})

test('a group the core reports unpinned keeps using the group endpoint', async ({
  onTestFinished,
}) => {
  const { invoke, testGroup } = await renderGroupTest(
    {
      cached: makePinnedSnapshot(),
      fresh: makeSnapshot(),
      delay: async () => ({ delay: 100 }),
    },
    onTestFinished,
  )
  await testGroup()
  expect(groupDelayCalls(invoke)).toHaveLength(1)
  expect(
    invoke.mock.calls.filter(
      ([method]) => method === 'clash_api_get_proxy_delay',
    ),
  ).toHaveLength(0)
})

test('a failed fresh read rejects the group test without testing anything', async ({
  onTestFinished,
}) => {
  const { invoke, hook } = await renderGroupTest(
    {
      cached: makePinnedSnapshot(),
      fresh: new Error('core unreachable'),
      delay: async () => ({ delay: 100 }),
    },
    onTestFinished,
  )
  await hook.act(async () => {
    await expect(
      hook.result.current.updateGroupDelay.mutateAsync(['group']),
    ).rejects.toThrow()
  })
  expect(groupDelayCalls(invoke)).toHaveLength(0)
  expect(
    invoke.mock.calls.filter(
      ([method]) => method === 'clash_api_get_proxy_delay',
    ),
  ).toHaveLength(0)
  expect(vi.getTimerCount()).toBe(0)
})

test('a failing member of a pinned group records a failed sample and the rest still run', async ({
  onTestFinished,
}) => {
  const { data } = await testPinnedGroup(async (params) => {
    if (params?.name === 'tested') throw new Error('timeout')
    return { delay: 100 }
  }, onTestFinished)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([42, 0])
  expect(data().nodes.other.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
})

test('a pinned group tests at most eight members at once', async ({
  onTestFinished,
}) => {
  const names = Array.from({ length: 10 }, (_, index) => `member-${index}`)
  const pinned = makePinnedSnapshot()
  pinned.groups[0] = { ...pinned.groups[0], all: names, fixed: names[0] }
  pinned.nodes = Object.fromEntries(names.map((name) => [name, node(name)]))

  const pending = new Map<string, PromiseWithResolvers<{ delay: number }>>()
  let inFlight = 0
  let peak = 0
  const { data, testGroup } = await renderGroupTest(
    {
      cached: pinned,
      fresh: pinned,
      delay: async (params) => {
        const name = params?.name as string
        const deferred = Promise.withResolvers<{ delay: number }>()
        pending.set(name, deferred)
        inFlight += 1
        peak = Math.max(peak, inFlight)
        try {
          return await deferred.promise
        } finally {
          inFlight -= 1
        }
      },
    },
    onTestFinished,
  )

  const finished = testGroup()
  await expect.poll(() => pending.size).toBe(8)
  expect(inFlight).toBe(8)

  // Settling one member, even by rejecting, frees a slot for the next.
  pending.get('member-0')!.reject(new Error('timeout'))
  await expect.poll(() => pending.size).toBe(9)
  pending.get('member-1')!.resolve({ delay: 100 })
  await expect.poll(() => pending.size).toBe(10)
  expect(peak).toBe(8)

  for (const name of names.slice(2)) {
    pending.get(name)!.resolve({ delay: 100 })
  }
  await finished

  expect(peak).toBe(8)
  expect(data().nodes['member-0'].history.map(({ delay }) => delay)).toEqual([
    42, 0,
  ])
  for (const name of names.slice(1)) {
    expect(data().nodes[name].history.map(({ delay }) => delay)).toEqual([
      42, 100,
    ])
  }
})
