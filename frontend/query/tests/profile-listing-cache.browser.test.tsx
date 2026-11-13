import type { PropsWithChildren } from 'react'
import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type {
  ProfileItem_Serialize,
  ProfileLayout,
  ProfileListingItem_Serialize,
  ProxyGroupLayout,
} from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  useCurrentProfileUid,
  useProfile,
  useProfileListing,
} from '../src/ipc/use-profile'
import { MutationProvider } from '../src/provider/mutation-provider'
import { RpcProvider } from '../src/provider/rpc-provider'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const fullConfig = (uid: string): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: uid,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const listingConfig = (uid: string): ProfileListingItem_Serialize => ({
  type: 'config',
  uid,
  name: uid,
  custom_name: false,
  has_desc: false,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed' },
    },
  },
})

const committed = { status: 'committed', value: null, commits: [] }

function makeClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
}

async function setupBothCaches(onTestFinished: TestContext['onTestFinished']) {
  let order = ['a', 'b']
  let folders: ProfileLayout['folders'] = []
  let layouts: Record<string, ProxyGroupLayout> = {}
  const pending: Array<{
    method: string
    settle: (value: unknown) => void
  }> = []

  const testRpc = createTestRpc({
    get_profiles: () => ({
      current: 'a',
      valid: [],
      global_transforms: [],
      folders,
      proxy_group_layouts: layouts,
      items: order.map(fullConfig),
    }),
    get_profiles_listing: () => ({
      current: 'a',
      global_transforms: [],
      folders,
      proxy_group_layouts: layouts,
      items: order.map(listingConfig),
    }),
    set_profile_layout: (params) =>
      new Promise((resolve, reject) => {
        const layout = params?.layout as ProfileLayout
        pending.push({
          method: 'set_profile_layout',
          settle: (value) => {
            if (value === 'fail') return reject(new Error('layout failed'))
            folders = layout.folders
            order = [
              ...layout.folders.flatMap((folder) => folder.members),
              ...layout.ungrouped,
            ]
            resolve(value)
          },
        })
      }),
    set_proxy_group_layout: (params) =>
      new Promise((resolve, reject) => {
        const profile = params?.profile as string
        const layout = params?.layout as ProxyGroupLayout
        pending.push({
          method: 'set_proxy_group_layout',
          settle: (value) => {
            if (value === 'fail')
              return reject(new Error('group layout failed'))
            layouts = { ...layouts, [profile]: layout }
            resolve(value)
          },
        })
      }),
  })

  const client = makeClient()
  const hook = await renderHook(
    () => ({ full: useProfile(), listing: useProfileListing() }),
    { wrapper: rpcWrapper(testRpc.rpc, client) },
  )
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })

  await expect
    .poll(() => hook.result.current.full.query.data?.items.length)
    .toBe(2)
  await expect
    .poll(() => hook.result.current.listing.data?.items.length)
    .toBe(2)

  return { hook, pending, testRpc }
}

test('useCurrentProfileUid reads get_profiles_listing without invoking get_profiles', async ({
  onTestFinished,
}) => {
  const testRpc = createTestRpc({
    get_profiles_listing: () => ({
      current: 'a',
      global_transforms: [],
      folders: [],
      proxy_group_layouts: {},
      items: [listingConfig('a')],
    }),
  })
  const client = makeClient()
  const hook = await renderHook(() => useCurrentProfileUid(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })

  await expect.poll(() => hook.result.current).toBe('a')
  const methods = testRpc.invoke.mock.calls.map(([method]) => method)
  expect(methods).toContain('get_profiles_listing')
  expect(methods).not.toContain('get_profiles')
})

test('setLayout updates and rolls back both full and listing caches', async ({
  onTestFinished,
}) => {
  const { hook, pending } = await setupBothCaches(onTestFinished)
  const nextLayout: ProfileLayout = {
    folders: [{ id: 'work', name: 'Work', members: ['b'] }],
    ungrouped: ['a'],
  }

  await hook.act(() => {
    hook.result.current.full.setLayout.mutateAsync(nextLayout).catch(() => {})
  })
  await expect.poll(() => pending.length).toBe(1)

  expect(
    hook.result.current.full.query.data?.items.map((item) => item.uid),
  ).toEqual(['b', 'a'])
  expect(
    hook.result.current.listing.data?.items.map((item) => item.uid),
  ).toEqual(['b', 'a'])
  expect(hook.result.current.listing.data?.folders).toEqual(nextLayout.folders)

  await hook.act(async () => pending[0].settle('fail'))
  await expect
    .poll(() => hook.result.current.full.query.data?.folders)
    .toEqual([])
  expect(hook.result.current.listing.data?.folders).toEqual([])
  expect(
    hook.result.current.listing.data?.items.map((item) => item.uid),
  ).toEqual(['a', 'b'])

  await hook.act(() => {
    hook.result.current.full.setLayout.mutateAsync(nextLayout).catch(() => {})
  })
  await expect.poll(() => pending.length).toBe(2)
  await hook.act(async () => pending[1].settle(committed))
  await expect
    .poll(() => hook.result.current.listing.data?.folders)
    .toEqual(nextLayout.folders)
})

test('setProxyGroupLayout updates and rolls back both full and listing caches', async ({
  onTestFinished,
}) => {
  const { hook, pending } = await setupBothCaches(onTestFinished)
  const groupLayout: ProxyGroupLayout = {
    folders: [{ id: 'media', name: 'Media', members: ['Streaming'] }],
    ungrouped: ['Proxy'],
  }

  await hook.act(() => {
    hook.result.current.full.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout: groupLayout })
      .catch(() => {})
  })
  await expect.poll(() => pending.length).toBe(1)

  expect(hook.result.current.full.query.data?.proxy_group_layouts).toEqual({
    a: groupLayout,
  })
  expect(hook.result.current.listing.data?.proxy_group_layouts).toEqual({
    a: groupLayout,
  })

  await hook.act(async () => pending[0].settle('fail'))
  await expect
    .poll(() => hook.result.current.listing.data?.proxy_group_layouts)
    .toEqual({})
  expect(hook.result.current.full.query.data?.proxy_group_layouts).toEqual({})

  await hook.act(() => {
    hook.result.current.full.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout: groupLayout })
      .catch(() => {})
  })
  await expect.poll(() => pending.length).toBe(2)
  await hook.act(async () => pending[1].settle(committed))
  await expect
    .poll(() => hook.result.current.listing.data?.proxy_group_layouts)
    .toEqual({ a: groupLayout })
})

test('MutationProvider refreshes both full and listing caches on profiles event and resync', async ({
  onTestFinished,
}) => {
  let version = 1
  const testRpc = createTestRpc({
    get_profiles: () => ({
      current: version === 1 ? 'a' : 'b',
      valid: [],
      global_transforms: [],
      folders: [],
      proxy_group_layouts: {},
      items: [fullConfig('a'), fullConfig('b')],
    }),
    get_profiles_listing: () => ({
      current: version === 1 ? 'a' : 'b',
      global_transforms: [],
      folders: [],
      proxy_group_layouts: {},
      items: [listingConfig('a'), listingConfig('b')],
    }),
  })
  const client = makeClient()
  const wrapper = ({ children }: PropsWithChildren) => (
    <RpcProvider rpc={testRpc.rpc}>
      <QueryClientProvider client={client}>
        <MutationProvider>{children}</MutationProvider>
      </QueryClientProvider>
    </RpcProvider>
  )
  const hook = await renderHook(
    () => ({ full: useProfile(), listing: useProfileListing() }),
    { wrapper },
  )
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })

  await expect
    .poll(() => hook.result.current.full.query.data?.current)
    .toBe('a')
  await expect.poll(() => hook.result.current.listing.data?.current).toBe('a')

  version = 2
  await testRpc.emitStateChanged({ kind: 'profiles', files: [] })
  await expect
    .poll(() => hook.result.current.full.query.data?.current)
    .toBe('b')
  await expect.poll(() => hook.result.current.listing.data?.current).toBe('b')

  version = 1
  testRpc.emitResync()
  await expect
    .poll(() => hook.result.current.full.query.data?.current)
    .toBe('a')
  await expect.poll(() => hook.result.current.listing.data?.current).toBe('a')
})
