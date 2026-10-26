import type { PropsWithChildren } from 'react'
import { beforeEach, expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useKvStorage } from '../src/hooks/use-kv-storage'
import { KvStorageProvider } from '../src/provider/kv-storage-provider'
import { kvStorageQueryKey } from '../src/provider/kv-storage-state'
import { MutationProvider } from '../src/provider/mutation-provider'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

// Revisions are assigned when a write commits, as the backend does, so a
// test that reorders answers cannot pass with a backend that never would.
const backend = vi.hoisted(() => ({
  values: new Map<string, string>(),
  revision: 0,
  failRead: false,
  // The invoke itself fails, as a lost IPC does, instead of the backend.
  loseRead: false,
  failWrite: false,
  writes: [] as string[],
  // Each read and write waits for the next gate, if one is left.
  readGates: [] as Promise<void>[],
  writeGates: [] as Promise<void>[],
}))

beforeEach(() => {
  localStorage.clear()
  backend.values = new Map()
  backend.revision = 0
  backend.failRead = false
  backend.loseRead = false
  backend.failWrite = false
  backend.writes = []
  backend.readGates = []
  backend.writeGates = []
})

const gate = () => {
  let release = () => {}
  const promise = new Promise<void>((resolve) => {
    release = resolve
  })
  return { promise, release }
}

async function mountStorage<T>(
  useHook: () => T,
  onTestFinished: TestContext['onTestFinished'],
  options: {
    listenGates?: Promise<void>[]
    withMutationProvider?: boolean
  } = {},
) {
  const testRpc = createTestRpc(
    {
      get_storage_item: async (params) => {
        const key = params?.key as string
        const snapshot = {
          value: backend.values.get(key) ?? null,
          revision: backend.revision,
        }
        await backend.readGates.shift()
        if (backend.loseRead) throw new Error('read lost')
        if (backend.failRead) {
          // Simulate the backend's string error payload so the generated Result wrapper is exercised.
          // oxlint-disable-next-line no-throw-literal
          throw 'read failed'
        }
        return snapshot
      },
      set_storage_item: async (params) => {
        const key = params?.key as string
        const value = params?.value as string
        const writeGate = backend.writeGates.shift()
        backend.writes.push(value)
        await writeGate
        if (backend.failWrite) {
          // oxlint-disable-next-line no-throw-literal
          throw 'write failed'
        }
        backend.values.set(key, value)
        backend.revision += 1
        return backend.revision
      },
    },
    { beforeListen: () => options.listenGates?.shift() },
  )
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  // The provider stays while the hook under it unmounts and mounts again.
  let shown = true
  const Root = ({ children }: PropsWithChildren) => {
    const storage = <KvStorageProvider>{shown && children}</KvStorageProvider>
    return (
      <Wrapper>
        {options.withMutationProvider ? (
          <MutationProvider>{storage}</MutationProvider>
        ) : (
          storage
        )}
      </Wrapper>
    )
  }
  const hook = await renderHook(useHook, { wrapper: Root })
  const remount = async () => {
    shown = false
    await hook.rerender()
    shown = true
    await hook.rerender()
  }
  onTestFinished(async () => {
    await hook.unmount()
    testRpc.rpc.dispose()
    client.clear()
  })
  const reads = () =>
    testRpc.invoke.mock.calls.filter(
      ([method]) => method === 'get_storage_item',
    ).length
  // What the backend publishes after a commit: the stored bytes are the JSON
  // encoding of the value's JSON text.
  const emitChange = (key: string, value: unknown, revision: number) =>
    testRpc.rpc.events.storageValueChangedEvent.emit({
      key: `web:${key}`,
      value: value === null ? null : JSON.stringify(JSON.stringify(value)),
      revision,
    })
  return { hook, testRpc, client, reads, emitChange, remount }
}

// Runs every callback already queued, such as registrations just released.
const settle = () => new Promise((resolve) => setTimeout(resolve))

const cache = (key: string, value: unknown) =>
  localStorage.setItem(`nyanpasu-kv-:${btoa(key)}`, JSON.stringify(value))

test('a backend value equal to the cache keeps the cached state', async ({
  onTestFinished,
}) => {
  cache('layout', { a: [1, 2] })
  backend.values.set('layout', JSON.stringify({ a: [1, 2] }))
  let renders = 0
  const { hook } = await mountStorage(() => {
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
  backend.values.set('layout', JSON.stringify({ a: [2] }))
  const { hook } = await mountStorage(
    () => useKvStorage('layout', {}),
    onTestFinished,
  )

  await expect.poll(() => hook.result.current[0]).toEqual({ a: [2] })
  expect(localStorage.getItem(`nyanpasu-kv-:${btoa('layout')}`)).toBe(
    JSON.stringify({ a: [2] }),
  )
})

test('migration validates local cache before the authoritative read', async ({
  onTestFinished,
}) => {
  cache('config', { old: true })
  let initial: { enabled: boolean } | undefined
  const { hook } = await mountStorage(() => {
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

test('hooks of a key share one read and remounting reads nothing', async ({
  onTestFinished,
}) => {
  backend.values.set('config', JSON.stringify({ a: 1 }))
  const { hook, reads, remount } = await mountStorage(
    () => [useKvStorage('config', {}), useKvStorage('config', {})] as const,
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)
  expect(hook.result.current[1][0]).toEqual({ a: 1 })

  await remount()
  expect(hook.result.current[0][0]).toEqual({ a: 1 })
  expect(hook.result.current[0][2].isLoading).toBe(false)
  expect(reads()).toBe(1)
})

test('a key switched to with the same text decodes with its own migration', async ({
  onTestFinished,
}) => {
  backend.values.set('a', '1')
  backend.values.set('b', '1')
  let key = 'a'
  const { hook } = await mountStorage(
    () => useKvStorage(key, '', { migrate: (value) => `${key}:${value}` }),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0]).toBe('a:1')

  key = 'b'
  await hook.rerender()
  await expect.poll(() => hook.result.current[0]).toBe('b:1')

  // Both keys are cached with the same text now.
  key = 'a'
  await hook.rerender()
  expect(hook.result.current[0]).toBe('a:1')
})

test('switching keys shows each key and reads each once', async ({
  onTestFinished,
}) => {
  backend.values.set('a', '"a"')
  backend.values.set('b', '"b"')
  let key = 'a'
  const { hook, reads } = await mountStorage(
    () => useKvStorage(key, ''),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0]).toBe('a')

  key = 'b'
  await hook.rerender()
  await expect.poll(() => hook.result.current[0]).toBe('b')
  key = 'a'
  await hook.rerender()
  expect(hook.result.current[0]).toBe('a')
  expect(hook.result.current[2].isLoading).toBe(false)
  expect(reads()).toBe(2)
})

test('the listeners are registered once for every hook', async ({
  onTestFinished,
}) => {
  const { hook, testRpc, remount } = await mountStorage(
    () => [useKvStorage('a', 0), useKvStorage('b', 0), useKvStorage('b', 0)],
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)
  await remount()

  expect(testRpc.listenerCount('storage-value-changed-event')).toBe(1)
  expect(testRpc.listenerCount('storage-resync-event')).toBe(1)
})

test('the first read waits for the listeners, so a later change is heard', async ({
  onTestFinished,
}) => {
  const changes = gate()
  const resyncs = gate()
  const { hook, reads, emitChange } = await mountStorage(
    () => useKvStorage('config', 'old'),
    onTestFinished,
    { listenGates: [changes.promise, resyncs.promise] },
  )
  changes.release()
  await settle()
  expect(reads()).toBe(0)

  resyncs.release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  await emitChange('config', 'new', 1)

  await expect.poll(() => hook.result.current[0]).toBe('new')
})

test('read failure blocks consumers until explicit refresh succeeds', async ({
  onTestFinished,
}) => {
  backend.failRead = true
  const { hook, reads, remount } = await mountStorage(
    () => useKvStorage('config', { fallback: true }),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].readError).toBe('read failed')
  expect(hook.result.current[0]).toEqual({ fallback: true })
  expect(hook.result.current[2].isLoading).toBe(false)
  // A remount does not retry on its own.
  await remount()
  expect(reads()).toBe(1)

  backend.failRead = false
  backend.values.set('config', JSON.stringify({ recovered: true }))
  await hook.result.current[2].refresh()
  await expect.poll(() => hook.result.current[0]).toEqual({ recovered: true })
  expect(hook.result.current[2].readError).toBeNull()
})

test('a delayed read cannot overwrite a newer completed write', async ({
  onTestFinished,
}) => {
  backend.values.set('config', JSON.stringify({ enabled: false }))
  const read = gate()
  backend.readGates = [read.promise]
  const { hook } = await mountStorage(
    () => useKvStorage('config', { enabled: false }),
    onTestFinished,
  )
  expect(await hook.result.current[1]({ enabled: true })).toBe(true)
  read.release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toEqual({ enabled: true })
})

test('an older event cannot replace a newer one', async ({
  onTestFinished,
}) => {
  const { hook, client, emitChange } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  await emitChange('config', 'b', 2)
  await expect.poll(() => hook.result.current[0]).toBe('b')
  await emitChange('config', 'a', 1)

  expect(client.getQueryData(kvStorageQueryKey('config'))).toMatchObject({
    value: '"b"',
    revision: 2,
  })
  expect(hook.result.current[0]).toBe('b')
})

test('functional updates of two hooks of a key build on each other', async ({
  onTestFinished,
}) => {
  const { hook } = await mountStorage(
    () =>
      [
        useKvStorage('config', { a: false, b: false }),
        useKvStorage('config', { a: false, b: false }),
      ] as const,
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)

  const [[, setFirst], [, setSecond]] = hook.result.current
  const first = setFirst((prev) => ({ ...prev, a: true }))
  const second = setSecond((prev) => ({ ...prev, b: true }))

  expect(await first).toBe(true)
  expect(await second).toBe(true)
  expect(JSON.parse(backend.values.get('config')!)).toEqual({
    a: true,
    b: true,
  })
  await expect
    .poll(() => hook.result.current[0][0])
    .toEqual({ a: true, b: true })
  expect(hook.result.current[1][0]).toEqual({ a: true, b: true })
})

test('writes of two hooks of a key reach the backend in call order', async ({
  onTestFinished,
}) => {
  const firstWrite = gate()
  backend.writeGates = [firstWrite.promise]
  const { hook } = await mountStorage(
    () =>
      [
        useKvStorage('config', { sort: 'default', hide: false }),
        useKvStorage('config', { sort: 'default', hide: false }),
      ] as const,
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)

  const first = hook.result.current[0][1]((prev) => ({ ...prev, sort: 'name' }))
  const second = hook.result.current[1][1]((prev) => ({ ...prev, hide: true }))

  // Both hooks show the newest value at once, and the second write waits.
  await expect
    .poll(() => hook.result.current[1][0])
    .toEqual({ sort: 'name', hide: true })
  await expect.poll(() => backend.writes.length).toBe(1)
  firstWrite.release()

  expect(await first).toBe(true)
  expect(await second).toBe(true)
  expect(backend.writes.map((write) => JSON.parse(write))).toEqual([
    { sort: 'name', hide: false },
    { sort: 'name', hide: true },
  ])
  await expect
    .poll(() => hook.result.current[0][0])
    .toEqual({ sort: 'name', hide: true })
})

test('writes of different keys do not wait for each other', async ({
  onTestFinished,
}) => {
  backend.writeGates = [gate().promise]
  const { hook } = await mountStorage(
    () => [useKvStorage('a', 0), useKvStorage('b', 0)] as const,
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)

  hook.result.current[0][1](1)
  expect(await hook.result.current[1][1](2)).toBe(true)
})

test('write failure is observable and retry writes the latest optimistic snapshot', async ({
  onTestFinished,
}) => {
  const { hook } = await mountStorage(
    () =>
      [
        useKvStorage('config', { a: false, b: false }),
        useKvStorage('config', { a: false, b: false }),
      ] as const,
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[0][2].isLoading).toBe(false)
  backend.failWrite = true
  expect(
    await hook.result.current[0][1]((prev) => ({ ...prev, a: true })),
  ).toBe(false)
  // The failed value stays shown in every hook of the key.
  await expect
    .poll(() => hook.result.current[1][0])
    .toEqual({ a: true, b: false })

  backend.failWrite = false
  expect(
    await hook.result.current[1][1]((prev) => ({ ...prev, b: true })),
  ).toBe(true)
  await expect
    .poll(() => hook.result.current[0][0])
    .toEqual({ a: true, b: true })
  expect(JSON.parse(backend.values.get('config')!)).toEqual({
    a: true,
    b: true,
  })
})

test('a newer commit from elsewhere replaces a failed value', async ({
  onTestFinished,
}) => {
  const { hook, emitChange } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  backend.failWrite = true
  expect(await hook.result.current[1]('local')).toBe(false)

  await expect.poll(() => hook.result.current[0]).toBe('local')
  await emitChange('config', 'remote', 7)

  await expect.poll(() => hook.result.current[0]).toBe('remote')
})

test('an earlier acknowledgement does not hide a newer pending write', async ({
  onTestFinished,
}) => {
  const secondWrite = gate()
  backend.writeGates = [Promise.resolve(), secondWrite.promise]
  const { hook, client, emitChange } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  const first = hook.result.current[1]('first')
  const second = hook.result.current[1]('second')
  expect(await first).toBe(true)
  // The echo of the first write arrives while the second is pending.
  await emitChange('config', 'first', 1)
  expect(client.getQueryData(kvStorageQueryKey('config'))).toMatchObject({
    value: '"first"',
    draft: { value: '"second"', failed: false },
  })
  await expect.poll(() => hook.result.current[0]).toBe('second')

  secondWrite.release()
  expect(await second).toBe(true)
  await emitChange('config', 'second', 2)
  expect(client.getQueryData(kvStorageQueryKey('config'))).toMatchObject({
    value: '"second"',
    draft: null,
  })
  expect(hook.result.current[0]).toBe('second')
})

test('an echo before its acknowledgement keeps the shown value', async ({
  onTestFinished,
}) => {
  const write = gate()
  backend.writeGates = [write.promise]
  let renders = 0
  const { hook, emitChange } = await mountStorage(() => {
    renders += 1
    return useKvStorage('config', { a: 0 })
  }, onTestFinished)
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  const pending = hook.result.current[1]({ a: 1 })
  await expect.poll(() => hook.result.current[0]).toEqual({ a: 1 })
  const shown = hook.result.current[0]
  const rendersAfterWrite = renders
  await emitChange('config', { a: 1 }, 1)
  write.release()
  expect(await pending).toBe(true)

  expect(hook.result.current[0]).toBe(shown)
  expect(renders).toBe(rendersAfterWrite)
})

test('a storage resync rereads every key used so far without a mount', async ({
  onTestFinished,
}) => {
  backend.values.set('a', '"a1"')
  backend.values.set('b', '"b1"')
  let showB = true
  const { hook, reads, testRpc } = await mountStorage(
    () => [useKvStorage('a', ''), useKvStorage(showB ? 'b' : 'a', '')],
    onTestFinished,
  )
  await expect.poll(() => hook.result.current[1][2].isLoading).toBe(false)
  expect(reads()).toBe(2)
  // `b` is no longer shown, but stays cached.
  showB = false
  await hook.rerender()

  // Commits whose events were lost in the gap.
  backend.values.set('a', '"a2"')
  backend.values.set('b', '"b2"')
  backend.revision = 5
  await testRpc.rpc.events.storageResyncEvent.emit({})

  await expect.poll(() => hook.result.current[0][0]).toBe('a2')
  expect(reads()).toBe(4)
  showB = true
  await hook.rerender()
  expect(hook.result.current[1][0]).toBe('b2')
  expect(reads()).toBe(4)
})

test("a transport resync rereads storage without the query client's refetch", async ({
  onTestFinished,
}) => {
  backend.values.set('config', '"before"')
  const { hook, reads, testRpc } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
    { withMutationProvider: true },
  )
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  backend.values.set('config', '"after"')
  backend.revision = 3
  testRpc.emitResync()

  await expect.poll(() => hook.result.current[0]).toBe('after')
  expect(reads()).toBe(2)
})

test('a read that a resync replaced does not settle the key', async ({
  onTestFinished,
}) => {
  backend.values.set('config', '"old"')
  const first = gate()
  const second = gate()
  backend.readGates = [first.promise, second.promise]
  const { hook, testRpc } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  await expect.poll(() => backend.readGates.length).toBe(1)

  backend.values.set('config', '"new"')
  backend.revision = 1
  await testRpc.rpc.events.storageResyncEvent.emit({})
  first.release()
  await settle()
  expect(hook.result.current[2].isLoading).toBe(true)

  second.release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toBe('new')
})

test('a failing read that a resync replaced does not fail the key', async ({
  onTestFinished,
}) => {
  backend.values.set('config', '"value"')
  const first = gate()
  const second = gate()
  backend.readGates = [first.promise, second.promise]
  const { hook, testRpc } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  await expect.poll(() => backend.readGates.length).toBe(1)
  await testRpc.rpc.events.storageResyncEvent.emit({})
  await expect.poll(() => backend.readGates.length).toBe(0)

  second.release()
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  backend.loseRead = true
  first.release()
  await settle()

  expect(hook.result.current[2].readError).toBeNull()
  expect(hook.result.current[0]).toBe('value')
})

test('unreadable stored JSON is a read error, not the default', async ({
  onTestFinished,
}) => {
  backend.values.set('config', '{')
  const { hook } = await mountStorage(
    () => useKvStorage('config', { fallback: true }),
    onTestFinished,
  )

  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[2].readError).toBeInstanceOf(SyntaxError)
})

test('an unreadable cache falls back to the default without an error', async ({
  onTestFinished,
}) => {
  localStorage.setItem(`nyanpasu-kv-:${btoa('config')}`, '{')
  const { hook } = await mountStorage(
    () => useKvStorage('config', { fallback: true }),
    onTestFinished,
  )

  expect(hook.result.current[0]).toEqual({ fallback: true })
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[2].readError).toBeNull()
})

test('an unreadable value older than the shown one is not an error', async ({
  onTestFinished,
}) => {
  backend.values.set('config', '{')
  const read = gate()
  backend.readGates = [read.promise]
  const { hook } = await mountStorage(
    () => useKvStorage('config', ''),
    onTestFinished,
  )
  expect(await hook.result.current[1]('valid')).toBe(true)

  read.release()

  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[2].readError).toBeNull()
  expect(hook.result.current[0]).toBe('valid')
})
