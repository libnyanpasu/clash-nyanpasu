import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { ProxyGroupLayout } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { useProfile } from '../src/ipc/use-profile'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const committed = { status: 'committed', value: null, commits: [] }

type Commit = {
  profile: string
  layout: ProxyGroupLayout
  settle: (value: unknown) => void
}

async function setup(onTestFinished: TestContext['onTestFinished']) {
  let document: {
    current: string
    valid: string[]
    global_transforms: string[]
    items: never[]
    folders: never[]
    proxy_group_layouts: Record<string, ProxyGroupLayout>
  } = {
    current: 'a',
    valid: [],
    global_transforms: [],
    items: [],
    folders: [],
    proxy_group_layouts: {},
  }
  const commits: Commit[] = []
  const testRpc = createTestRpc({
    get_profiles: async () => document,
    set_proxy_group_layout: (params) =>
      new Promise((resolve, reject) => {
        const profile = params?.profile as string
        const layout = params?.layout as ProxyGroupLayout
        commits.push({
          profile,
          layout,
          settle: (value) => {
            if (value === 'fail') return reject(new Error('rejected'))
            const { [profile]: _, ...rest } = document.proxy_group_layouts
            document = {
              ...document,
              proxy_group_layouts:
                layout.folders.length + layout.ungrouped.length === 0
                  ? rest
                  : { ...rest, [profile]: layout },
            }
            resolve(value)
          },
        })
      }),
  })
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const hook = await renderHook(() => useProfile(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })
  await expect.poll(() => hook.result.current.query.data?.current).toBe('a')
  return { hook, commits }
}

const layout: ProxyGroupLayout = {
  folders: [{ id: 'f1', name: 'Streaming', members: ['Netflix'] }],
  ungrouped: ['Proxy'],
}

const stored = (hook: Awaited<ReturnType<typeof setup>>['hook']) =>
  hook.result.current.query.data?.proxy_group_layouts

test('an arrangement shows in the cache before the commit returns', async ({
  onTestFinished,
}) => {
  const { hook, commits } = await setup(onTestFinished)
  expect(stored(hook)).toEqual({})

  await hook.act(() => {
    hook.result.current.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout })
      .catch(() => {})
  })

  await expect.poll(() => commits.length).toBe(1)
  expect(commits[0].profile).toBe('a')
  expect(stored(hook)).toEqual({ a: layout })

  await hook.act(async () => commits[0].settle(committed))
  await expect
    .poll(() => hook.result.current.setProxyGroupLayout.isSuccess)
    .toBe(true)
  expect(stored(hook)).toEqual({ a: layout })
})

test('a rejected arrangement rolls the cache back', async ({
  onTestFinished,
}) => {
  const { hook, commits } = await setup(onTestFinished)

  await hook.act(() => {
    hook.result.current.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout })
      .catch(() => {})
  })
  await expect.poll(() => commits.length).toBe(1)
  expect(stored(hook)).toEqual({ a: layout })

  await hook.act(async () => commits[0].settle('fail'))

  await expect
    .poll(() => hook.result.current.setProxyGroupLayout.isError)
    .toBe(true)
  await expect.poll(() => stored(hook)).toEqual({})
})

test('an empty arrangement removes the entry, and commits are queued', async ({
  onTestFinished,
}) => {
  const { hook, commits } = await setup(onTestFinished)
  const empty: ProxyGroupLayout = { folders: [], ungrouped: [] }

  await hook.act(() => {
    hook.result.current.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout })
      .catch(() => {})
    hook.result.current.setProxyGroupLayout
      .mutateAsync({ profile: 'a', layout: empty })
      .catch(() => {})
  })

  await expect.poll(() => commits.length).toBe(1)
  expect(commits).toHaveLength(1)

  await hook.act(async () => commits[0].settle(committed))
  await expect.poll(() => commits.length).toBe(2)
  expect(commits[1].layout).toEqual(empty)

  await hook.act(async () => commits[1].settle(committed))
  await expect.poll(() => stored(hook)).toEqual({})
})
