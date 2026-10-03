import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import {
  createRpcClient,
  type RpcEventCallback,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import type { AppUpdateSnapshot } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  acceptAppUpdateSnapshot,
  APP_UPDATE_QUERY_KEY,
  useAppUpdate,
} from '../src/ipc/use-app-update'
import { rpcWrapper } from './rpc-test-utils'

type EventHandler = RpcEventCallback<AppUpdateSnapshot>

function snapshot(revision: number): AppUpdateSnapshot {
  return {
    revision,
    phase: 'idle',
    release: null,
    downloaded: 0,
    total: null,
    speed: 0,
    source: null,
    error: null,
    last_checked_at: null,
    supported: true,
    endpoints: [],
  }
}

function makeRpc(
  getSnapshot: () => Promise<AppUpdateSnapshot>,
  listenGate?: Promise<void>,
) {
  let handler: EventHandler | undefined
  const listeners = new Set<() => void>()
  const unlisten = vi.fn(() => {
    handler = undefined
  })
  const invoke = vi.fn(
    async (method: string, _params?: Record<string, unknown>) => {
      if (method === 'get_app_update_state') return getSnapshot()
      throw new Error(`Unexpected RPC method: ${method}`)
    },
  )
  const commands = {
    invoke: async <T,>(method: string, params?: Record<string, unknown>) =>
      (await invoke(method, params)) as T,
  }
  const events: RpcEventTransport = {
    listen: vi.fn(async <T,>(_name: string, callback: RpcEventCallback<T>) => {
      handler = callback as EventHandler
      await listenGate
      return unlisten
    }),
    once: vi.fn(async () => () => {}),
    emit: vi.fn(async () => {}),
    listenMutation: vi.fn(async () => () => {}),
    listenResync: vi.fn((callback: () => void) => {
      listeners.add(callback)
      return () => listeners.delete(callback)
    }),
    dispose: vi.fn(),
  }
  return {
    rpc: createRpcClient({ commands, events }),
    invoke,
    listen: events.listen,
    unlisten,
    emit(next: AppUpdateSnapshot) {
      handler?.({
        event: 'app-update-state-changed',
        id: next.revision,
        payload: next,
      })
    },
    resync() {
      for (const listener of listeners) listener()
    },
  }
}

async function setup(
  getSnapshot: () => Promise<AppUpdateSnapshot>,
  onTestFinished: TestContext['onTestFinished'],
  enabled = true,
  listenGate?: Promise<void>,
) {
  const mock = makeRpc(getSnapshot, listenGate)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const hook = await renderHook(() => useAppUpdate({ enabled }), {
    wrapper: rpcWrapper(mock.rpc, queryClient),
  })
  onTestFinished(async () => {
    await hook.unmount()
    mock.rpc.dispose()
    queryClient.clear()
  })
  return { ...mock, hook, queryClient }
}

function deferred<T>() {
  let complete!: (value: T) => void
  const promise = new Promise<T>((resolve) => {
    complete = resolve
  })
  return { promise, resolve: complete }
}

test('keeps the cached snapshot when an event repeats its revision', () => {
  const current = snapshot(7)
  const conflicting = snapshot(7)
  expect(acceptAppUpdateSnapshot(current, conflicting)).toBe(current)
})

test('an update event wins over an older snapshot request already in flight', async ({
  onTestFinished,
}) => {
  const initial = deferred<AppUpdateSnapshot>()
  let requests = 0
  const { emit, hook, queryClient } = await setup(async () => {
    requests += 1
    if (requests === 1) return initial.promise
    return snapshot(3)
  }, onTestFinished)

  await expect.poll(() => requests).toBe(1)
  await expect.poll(() => hook.result.current.snapshot).toBeUndefined()
  emit(snapshot(3))
  await expect
    .poll(
      () =>
        queryClient.getQueryData<AppUpdateSnapshot>(APP_UPDATE_QUERY_KEY)
          ?.revision,
    )
    .toBe(3)

  initial.resolve(snapshot(1))
  await expect
    .poll(
      () =>
        queryClient.getQueryData<AppUpdateSnapshot>(APP_UPDATE_QUERY_KEY)
          ?.revision,
    )
    .toBe(3)
})

test('ignores older events, refetches on resync, and removes listeners on unmount', async ({
  onTestFinished,
}) => {
  let revision = 1
  const { emit, hook, invoke, listen, resync, unlisten } = await setup(
    async () => snapshot(revision),
    onTestFinished,
  )

  await expect.poll(() => hook.result.current.snapshot?.revision).toBe(1)
  await expect.poll(() => listen).toHaveBeenCalledTimes(1)
  emit(snapshot(4))
  await expect.poll(() => hook.result.current.snapshot?.revision).toBe(4)
  emit(snapshot(2))
  await expect.poll(() => hook.result.current.snapshot?.revision).toBe(4)

  revision = 5
  resync()
  await expect.poll(() => hook.result.current.snapshot?.revision).toBe(5)
  await expect.poll(() => invoke).toHaveBeenCalledTimes(2)

  await hook.unmount()
  expect(unlisten).toHaveBeenCalledTimes(1)
})

test('does not query or subscribe when desktop updates are disabled', async ({
  onTestFinished,
}) => {
  const mock = await setup(async () => snapshot(1), onTestFinished, false)

  await expect.poll(() => mock.invoke).not.toHaveBeenCalled()
  expect(mock.listen).not.toHaveBeenCalled()
})

test('unlistens when subscription setup finishes after unmount', async ({
  onTestFinished,
}) => {
  const listenGate = deferred<void>()
  const mock = await setup(
    async () => snapshot(1),
    onTestFinished,
    true,
    listenGate.promise,
  )

  await expect.poll(() => mock.invoke).toHaveBeenCalledTimes(1)
  await expect.poll(() => mock.listen).toHaveBeenCalledTimes(1)
  await mock.hook.unmount()

  listenGate.resolve()
  await expect.poll(() => mock.unlisten).toHaveBeenCalledTimes(1)
  expect(mock.invoke).toHaveBeenCalledTimes(1)
})
