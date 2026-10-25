import { expect, test } from 'vitest'
import { render, renderHook } from 'vitest-browser-react'
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
  // The mount, and the read once the listener registered.
  await expect.poll(fetches).toBe(2)

  await testRpc.emitStateChanged({ kind: 'nyanpasu_config' })

  await expect.poll(fetches).toBe(3)
})

test('a query its events cover is kept across mounts until an event arrives', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => ({ global: null, groups: [], nodes: {} }),
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const Probe = () => {
    useClashProxies()
    return null
  }
  const page = (mounted: boolean) => (
    <Wrapper>
      <MutationProvider>{mounted && <Probe />}</MutationProvider>
    </Wrapper>
  )
  const screen = await render(page(true))
  onTestFinished(async () => {
    await screen.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
  })
  const fetches = () =>
    testRpc.invoke.mock.calls.filter(([method]) => method === 'get_proxies')
      .length
  // The mount, and the read once the listener registered.
  await expect.poll(fetches).toBe(2)
  await expect.poll(() => client.isFetching()).toBe(0)

  await screen.rerender(page(false))
  await screen.rerender(page(true))
  expect(fetches()).toBe(2)

  await testRpc.emitStateChanged({ kind: 'proxies' })
  await expect.poll(fetches).toBe(3)
})

test('a change during a read is answered by a read that starts after it', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
    },
  })
  const answers: ((content: string) => void)[] = []
  const testRpc = createTestRpc({
    read_profile_file: () =>
      new Promise<string>((resolve) => answers.push(resolve)),
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook(() => useProfileContent('a').query, {
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
  await expect.poll(() => answers.length).toBe(1)

  await testRpc.emitStateChanged({ kind: 'profiles', files: ['a'] })
  await expect.poll(() => answers.length).toBe(2)
  answers[0]('before')
  answers[1]('after')

  await expect.poll(() => hook.result.current.data).toBe('after')
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

test('a resync during a read is answered by a read that starts after it', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
    },
  })
  const answers: ((content: string) => void)[] = []
  const testRpc = createTestRpc({
    read_profile_file: () =>
      new Promise<string>((resolve) => answers.push(resolve)),
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook(() => useProfileContent('a').query, {
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
  await expect.poll(() => answers.length).toBe(1)

  testRpc.emitResync()
  await expect.poll(() => answers.length).toBe(2)
  answers[0]('before')
  answers[1]('after')

  await expect.poll(() => hook.result.current.data).toBe('after')
})
