import { beforeEach, expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { useKvStorage } from '../src/hooks/use-kv-storage'

const backend = vi.hoisted(() => ({
  value: null as string | null,
  failRead: false,
  failWrite: false,
  writes: [] as string[],
  readGate: null as Promise<void> | null,
}))
vi.mock('../src/ipc/rpc', async (importOriginal) => {
  const { rpc } = await importOriginal<typeof import('../src/ipc/rpc')>()
  return {
    rpc: {
      ...rpc,
      queries: {
        ...rpc.queries,
        getStorageItem: () => ({
          queryFn: async () => {
            const value = backend.value
            await backend.readGate
            return backend.failRead
              ? { status: 'error', error: 'read failed' }
              : { status: 'ok', data: value }
          },
        }),
      },
      mutations: {
        ...rpc.mutations,
        setStorageItem: {
          mutationFn: async ([, value]: [string, string]) => {
            backend.writes.push(value)
            if (backend.failWrite)
              return { status: 'error', error: 'write failed' }
            backend.value = value
            return { status: 'ok', data: null }
          },
        },
      },
      listenResync: () => () => {},
      events: {
        ...rpc.events,
        storageValueChangedEvent: { listen: async () => () => {} },
      },
    },
  }
})

const cache = (key: string, value: unknown) =>
  localStorage.setItem(`nyanpasu-kv-:${btoa(key)}`, JSON.stringify(value))

test('a backend value equal to the cache keeps the cached state', async ({
  onTestFinished,
}) => {
  cache('layout', { a: [1, 2] })
  backend.value = JSON.stringify({ a: [1, 2] })
  let renders = 0
  const hook = await renderHook(() => {
    renders += 1
    return useKvStorage('layout', {})
  })
  onTestFinished(() => hook.unmount())
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
  const hook = await renderHook(() => useKvStorage('layout', {}))
  onTestFinished(() => hook.unmount())

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
  const hook = await renderHook(() => {
    const result = useKvStorage(
      'config',
      { enabled: true },
      { migrate: () => ({ enabled: false }) },
    )
    initial ??= result[0]
    return result
  })
  onTestFinished(() => hook.unmount())
  expect(initial).toEqual({ enabled: false })
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
})

test('write failure is observable and retry writes the latest optimistic snapshot', async ({
  onTestFinished,
}) => {
  const hook = await renderHook(() =>
    useKvStorage('config', { a: false, b: false }),
  )
  onTestFinished(() => hook.unmount())
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
  const hook = await renderHook(() => useKvStorage('config', {}))
  onTestFinished(() => hook.unmount())
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
  const hook = await renderHook(() =>
    useKvStorage('config', { enabled: false }),
  )
  onTestFinished(() => hook.unmount())
  expect(await hook.result.current[1]({ enabled: true })).toBe(true)
  release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toEqual({ enabled: true })
})
