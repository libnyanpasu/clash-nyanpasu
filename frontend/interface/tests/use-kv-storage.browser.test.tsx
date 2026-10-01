import { expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { useKvStorage } from '../src/hooks/use-kv-storage'

const backend = vi.hoisted(() => ({ value: null as string | null }))
vi.mock('../src/ipc/rpc', async (importOriginal) => {
  const { rpc } = await importOriginal<typeof import('../src/ipc/rpc')>()
  return {
    rpc: {
      ...rpc,
      queries: {
        ...rpc.queries,
        getStorageItem: () => ({
          queryFn: async () => ({ status: 'ok', data: backend.value }),
        }),
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
