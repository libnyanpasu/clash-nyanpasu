import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { ProfileItem_Serialize, ProfileLayout } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { useProfile } from '../src/ipc/use-profile'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const config = (uid: string): ProfileItem_Serialize => ({
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

const committed = { status: 'committed', value: null, commits: [] }

type Commit = { layout: ProfileLayout; settle: (value: unknown) => void }

async function setup(onTestFinished: TestContext['onTestFinished']) {
  let document = {
    current: null,
    valid: [],
    global_transforms: [],
    items: [config('a'), config('b')],
    folders: [] as ProfileLayout['folders'],
  }
  const commits: Commit[] = []
  const testRpc = createTestRpc({
    get_profiles: async () => document,
    set_profile_layout: (params) =>
      new Promise((resolve, reject) => {
        const layout = params?.layout as ProfileLayout
        commits.push({
          layout,
          settle: (value) => {
            if (value === 'fail') return reject(new Error('rejected'))
            document = {
              ...document,
              folders: layout.folders,
              items: [
                ...layout.folders.flatMap((folder) =>
                  folder.members.map((uid) => config(uid)),
                ),
                ...layout.ungrouped.map((uid) => config(uid)),
              ],
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
  await expect.poll(() => hook.result.current.query.data?.items.length).toBe(2)
  return { hook, commits }
}

const layout: ProfileLayout = {
  folders: [{ id: 'f1', name: 'Work', members: ['b'] }],
  ungrouped: ['a'],
}

const order = (hook: Awaited<ReturnType<typeof setup>>['hook']) =>
  hook.result.current.query.data?.items.map((item) => item.uid)

test('a layout shows in the cache before the commit returns', async ({
  onTestFinished,
}) => {
  const { hook, commits } = await setup(onTestFinished)
  expect(hook.result.current.query.data?.folders).toEqual([])

  await hook.act(() => {
    hook.result.current.setLayout.mutateAsync(layout).catch(() => {})
  })

  await expect.poll(() => commits.length).toBe(1)
  expect(hook.result.current.query.data?.folders).toEqual(layout.folders)
  expect(order(hook)).toEqual(['b', 'a'])

  await hook.act(async () => commits[0].settle(committed))
  await expect.poll(() => hook.result.current.setLayout.isSuccess).toBe(true)
  expect(hook.result.current.query.data?.folders).toEqual(layout.folders)
})

test('a rejected layout rolls the cache back', async ({ onTestFinished }) => {
  const { hook, commits } = await setup(onTestFinished)

  await hook.act(() => {
    hook.result.current.setLayout.mutateAsync(layout).catch(() => {})
  })
  await expect.poll(() => commits.length).toBe(1)
  expect(order(hook)).toEqual(['b', 'a'])

  await hook.act(async () => commits[0].settle('fail'))

  await expect.poll(() => hook.result.current.setLayout.isError).toBe(true)
  await expect.poll(() => order(hook)).toEqual(['a', 'b'])
  expect(hook.result.current.query.data?.folders).toEqual([])
})

test('queued layouts are committed one at a time', async ({
  onTestFinished,
}) => {
  const { hook, commits } = await setup(onTestFinished)
  const second: ProfileLayout = { folders: [], ungrouped: ['b', 'a'] }

  await hook.act(() => {
    hook.result.current.setLayout.mutateAsync(layout).catch(() => {})
    hook.result.current.setLayout.mutateAsync(second).catch(() => {})
  })

  await expect.poll(() => commits.length).toBe(1)
  expect(commits).toHaveLength(1)

  await hook.act(async () => commits[0].settle(committed))
  await expect.poll(() => commits.length).toBe(2)
  expect(commits[1].layout).toEqual(second)

  await hook.act(async () => commits[1].settle(committed))
  await expect.poll(() => order(hook)).toEqual(['b', 'a'])
  expect(hook.result.current.query.data?.folders).toEqual([])
})
