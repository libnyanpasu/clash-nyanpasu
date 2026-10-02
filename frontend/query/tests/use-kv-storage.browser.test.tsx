import { beforeEach, expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useKvStorage } from '../src/hooks/use-kv-storage'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const backend = vi.hoisted(() => ({
  value: null as string | null,
  failRead: false,
  failWrite: false,
  writes: [] as string[],
  readGate: null as Promise<void> | null,
}))
async function mountStorageHook<T>(
  useHook: () => T,
  onTestFinished: TestContext['onTestFinished'],
) {
  const testRpc = createTestRpc({
    get_storage_item: async () => {
      const value = backend.value
      await backend.readGate
      if (backend.failRead) {
        // Simulate the backend's string error payload so the generated Result wrapper is exercised.
        // oxlint-disable-next-line no-throw-literal
        throw 'read failed'
      }
      return value
    },
    set_storage_item: async (params) => {
      const value = params?.value as string
      backend.writes.push(value)
      if (backend.failWrite) {
        // Simulate the backend's string error payload so the generated Result wrapper is exercised.
        // oxlint-disable-next-line no-throw-literal
        throw 'write failed'
      }
      backend.value = value
      return null
    },
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const hook = await renderHook(useHook, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    testRpc.rpc.dispose()
    client.clear()
  })
  return hook
}

const cache = (key: string, value: unknown) =>
  localStorage.setItem(`nyanpasu-kv-:${btoa(key)}`, JSON.stringify(value))

test('a backend value equal to the cache keeps the cached state', async ({
  onTestFinished,
}) => {
  cache('layout', { a: [1, 2] })
  backend.value = JSON.stringify({ a: [1, 2] })
  let renders = 0
  const hook = await mountStorageHook(() => {
    renders += 1
    return useKvStorage('layout', {})
  }, onTestFinished)
  const cached = hook.result.current[0]

  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  expect(hook.result.current[0]).toBe(cached)
  // The first render and the one that ends loading.
  expect(renders).toBe(2)
})

test('a backend value that differs from the cache replaces it', async ({
  onTestFinished,
}) => {
  cache('layout', { a: [1] })
  backend.value = JSON.stringify({ a: [2] })
  const hook = await mountStorageHook(
    () => useKvStorage('layout', {}),
    onTestFinished,
  )

  await expect.poll(() => hook.result.current[0]).toEqual({ a: [2] })
  expect(localStorage.getItem(`nyanpasu-kv-:${btoa('layout')}`)).toBe(
    JSON.stringify({ a: [2] }),
  )
})

beforeEach(() => {
  localStorage.clear()
  backend.value = null
  backend.failRead = false
  backend.failWrite = false
  backend.writes = []
  backend.readGate = null
})

test('migration validates local cache before the authoritative read', async ({
  onTestFinished,
}) => {
  cache('config', { old: true })
  let initial: { enabled: boolean } | undefined
  const hook = await mountStorageHook(() => {
    const result = useKvStorage(
      'config',
      { enabled: true },
      { migrate: () => ({ enabled: false }) },
    )
    initial ??= result[0]
    return result
  }, onTestFinished)
  expect(initial).toEqual({ enabled: false })
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
})

test('write failure is observable and retry writes the latest optimistic snapshot', async ({
  onTestFinished,
}) => {
  const hook = await mountStorageHook(
    () => useKvStorage('config', { a: false, b: false }),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  backend.failWrite = true
  expect(await hook.result.current[1]((prev) => ({ ...prev, a: true }))).toBe(
    false,
  )
  backend.failWrite = false
  expect(await hook.result.current[1]((prev) => ({ ...prev, b: true }))).toBe(
    true,
  )
  await expect.poll(() => hook.result.current[0]).toEqual({ a: true, b: true })
  expect(JSON.parse(backend.value!)).toEqual({ a: true, b: true })
})

test('read failure blocks consumers until explicit refresh succeeds', async ({
  onTestFinished,
}) => {
  backend.failRead = true
  const hook = await mountStorageHook(
    () => useKvStorage('config', {}),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].readError).toBe('read failed')
  backend.failRead = false
  backend.value = JSON.stringify({ recovered: true })
  await hook.result.current[2].refresh()
  await expect.poll(() => hook.result.current[0]).toEqual({ recovered: true })
  expect(hook.result.current[2].readError).toBeNull()
})

test('a delayed read cannot overwrite a newer completed write', async ({
  onTestFinished,
}) => {
  backend.value = JSON.stringify({ enabled: false })
  let release = () => {}
  backend.readGate = new Promise<void>((resolve) => {
    release = resolve
  })
  const hook = await mountStorageHook(
    () => useKvStorage('config', { enabled: false }),
    onTestFinished,
  )
  expect(await hook.result.current[1]({ enabled: true })).toBe(true)
  release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toEqual({ enabled: true })
})
