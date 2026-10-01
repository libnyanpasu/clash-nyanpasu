import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import { useClashCores, useClashCoreVersion } from '../src/ipc/use-clash-cores'

const ipc = vi.hoisted(() => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  return {
    invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
  }
})
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  invoke: ipc.invoke,
}))

const versionCalls = () =>
  ipc.invoke.mock.calls.filter(
    ([, args]) => (args as { method: string }).method === 'get_core_version',
  ).length

function setup(onTestFinished: TestContext['onTestFinished']) {
  // The app's client keeps React Query's defaults.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  ipc.invoke.mockImplementation(async (_command, args) => {
    const { method } = args as { method: string }
    if (method === 'get_core_version') return 'v1.0.0'
    throw new Error(`Unexpected command: ${method}`)
  })
  onTestFinished(() => {
    client.clear()
    ipc.invoke.mockReset()
  })
  const mount = <T,>(useHook: () => T) =>
    renderHook(useHook, {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
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
  expect(ipc.invoke).toHaveBeenCalledWith('call_rpc', {
    method: 'get_core_version',
    params: { coreType: 'mihomo' },
  })

  ipc.invoke.mockImplementation(async () => 'v2.0.0')
  await hook.result.current.cores.refetchVersions()

  await expect.poll(() => hook.result.current.version.data).toBe('v2.0.0')
  await expect
    .poll(() => hook.result.current.cores.query.data?.mihomo.currentVersion)
    .toBe('v2.0.0')
})
