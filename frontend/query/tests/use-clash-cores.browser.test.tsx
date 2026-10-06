import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { focusManager, QueryClient } from '@tanstack/react-query'
import { useClashCores, useClashCoreVersion } from '../src/ipc/use-clash-cores'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const versionCalls = () =>
  testRpc.invoke.mock.calls.filter(([method]) => method === 'get_core_version')
    .length
let testRpc: ReturnType<typeof createTestRpc>

function setup(onTestFinished: TestContext['onTestFinished']) {
  // The app's client keeps React Query's defaults.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  testRpc = createTestRpc({
    get_core_version: async () => 'v1.0.0',
  })
  onTestFinished(() => {
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })
  const mount = <T,>(useHook: () => T) =>
    renderHook(useHook, {
      wrapper: rpcWrapper(testRpc.rpc, client),
    })
  return { mount }
}

test('core versions are read once, not on every visit or focus', async ({
  onTestFinished,
}) => {
  const { mount } = setup(onTestFinished)

  const first = await mount(() => useClashCores().query)
  await expect.poll(() => first.result.current.isSuccess).toBe(true)
  const calls = versionCalls()
  await first.unmount()

  const second = await mount(() => useClashCores().query)
  onTestFinished(() => second.unmount())
  focusManager.setFocused(false)
  focusManager.setFocused(true)
  await new Promise((resolve) => setTimeout(resolve, 100))

  expect(second.result.current.data?.mihomo.currentVersion).toBe('v1.0.0')
  expect(versionCalls()).toBe(calls)
})

test('one core version is read alone and refetched with the others', async ({
  onTestFinished,
}) => {
  const { mount } = setup(onTestFinished)

  const hook = await mount(() => ({
    version: useClashCoreVersion('mihomo'),
    cores: useClashCores(),
  }))
  onTestFinished(() => hook.unmount())
  await expect.poll(() => hook.result.current.version.data).toBe('v1.0.0')
  expect(testRpc.invoke).toHaveBeenCalledWith('get_core_version', {
    coreType: 'mihomo',
  })

  testRpc.invoke.mockImplementation(async () => 'v2.0.0')
  await hook.result.current.cores.refetchVersions()

  await expect.poll(() => hook.result.current.version.data).toBe('v2.0.0')
  await expect
    .poll(() => hook.result.current.cores.query.data?.mihomo.currentVersion)
    .toBe('v2.0.0')
})

test('a failed core version read stays unavailable without looking up to date', async ({
  onTestFinished,
}) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  testRpc = createTestRpc({
    get_core_version: async () => {
      throw new Error('sidecar could not run')
    },
  })
  onTestFinished(() => {
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })
  const hook = await renderHook(() => useClashCores().query, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(() => hook.unmount())

  await expect.poll(() => hook.result.current.isSuccess).toBe(true)
  expect(hook.result.current.data?.mihomo).toMatchObject({
    currentVersion: 'N/A',
    versionReadError: true,
  })
})
