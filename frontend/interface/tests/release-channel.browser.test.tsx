import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useReleaseChannel } from '../src/ipc/use-release-channel'

const ipc = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
}))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  invoke: ipc.invoke,
}))

async function setup(onTestFinished: TestContext['onTestFinished']) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  ipc.invoke.mockImplementation(async (command) => {
    if (command === 'get_release_channel') return 'stable'
    throw new Error(`Unexpected command: ${command}`)
  })
  const hook = await renderHook(() => useReleaseChannel(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    ipc.invoke.mockReset()
  })
  await expect.poll(() => hook.result.current.query.data).toBe('stable')
  return { hook, client }
}

test('channel changes are displayed only after persistence succeeds', async ({
  onTestFinished,
}) => {
  const { hook } = await setup(onTestFinished)
  let commit!: (value: unknown) => void
  ipc.invoke.mockImplementation(
    () =>
      new Promise((resolve) => {
        commit = resolve
      }),
  )
  let pending!: Promise<unknown>
  await hook.act(() => {
    pending = hook.result.current.mutation.mutateAsync('beta')
  })
  await expect.poll(() => hook.result.current.mutation.isPending).toBe(true)
  expect(hook.result.current.query.data).toBe('stable')
  await hook.act(async () => {
    commit({ status: 'applied', value: null })
    await pending
  })
  await expect.poll(() => hook.result.current.query.data).toBe('beta')
  expect(ipc.invoke).toHaveBeenCalledWith('set_release_channel', {
    channel: 'beta',
  })
})

test('backend rejection keeps the committed nightly channel visible', async ({
  onTestFinished,
}) => {
  const { hook, client } = await setup(onTestFinished)
  await hook.act(() => {
    client.setQueryData(['getReleaseChannel'], 'nightly')
  })
  ipc.invoke.mockRejectedValue('cannot leave the nightly release channel')
  await hook.act(async () => {
    await expect(
      hook.result.current.mutation.mutateAsync('stable'),
    ).rejects.toBeDefined()
  })
  expect(hook.result.current.query.data).toBe('nightly')
})
