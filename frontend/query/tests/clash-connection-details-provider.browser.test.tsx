import {
  afterEach,
  beforeEach,
  expect,
  test,
  vi,
  type TestContext,
} from 'vitest'
import { render } from 'vitest-browser-react'
import { createRpcClient } from '@nyanpasu/rpc'
import type { ClashConnectionDetails_Serialize } from '@nyanpasu/rpc/types'
import { Channel } from '@tauri-apps/api/core'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import {
  ClashConnectionDetailsFreezeBoundary,
  ClashConnectionDetailsProvider,
  useClashConnectionDetails,
} from '../src/provider/clash-connection-details-provider'
import { RpcProvider } from '../src/provider/rpc-provider'

const details = (sequence: number): ClashConnectionDetails_Serialize => ({
  sequence,
  connections: [],
})

// `mockIPC` wires up the real `window.__TAURI_INTERNALS__` plumbing
// (`invoke`, `transformCallback`, `runCallback`), so a real `Channel` can be
// constructed and delivered to, the same as the production Tauri runtime.
function nextIndex() {
  const seen = new WeakMap<Channel<ClashConnectionDetails_Serialize>, number>()
  return (channel: Channel<ClashConnectionDetails_Serialize>) => {
    const index = seen.get(channel) ?? 0
    seen.set(channel, index + 1)
    return index
  }
}

type RpcCall = { method: string; params: Record<string, unknown> }

function mockRpcCalls(
  handleCall: (method: string, params: Record<string, unknown>) => unknown,
) {
  mockIPC((cmd, args) => {
    expect(cmd).toBe('call_rpc')
    expect(Object.keys(args ?? {}).sort()).toEqual(['method', 'params'])
    expect(args).toEqual(
      expect.objectContaining({
        method: expect.any(String),
        params: expect.any(Object),
      }),
    )
    const { method, params } = args as RpcCall
    if (method === 'subscribe_clash_connection_details') {
      const channel =
        params.onFrame as Channel<ClashConnectionDetails_Serialize>
      expect(channel).toBeInstanceOf(Channel)
      expect(JSON.parse(JSON.stringify(args))).toEqual({
        method,
        params: { onFrame: `__CHANNEL__:${channel.id}` },
      })
    }
    return handleCall(method, params)
  })
}

// Resolves subscribe calls with sequential ids and captures each channel, so
// a test can push a frame or assert a subscription ended.
function mockSubscriptions() {
  let nextId = 0
  const channels = new Map<number, Channel<ClashConnectionDetails_Serialize>>()
  const unsubscribed: number[] = []
  const index = nextIndex()

  mockRpcCalls((method, params) => {
    if (method === 'subscribe_clash_connection_details') {
      expect(Object.keys(params)).toEqual(['onFrame'])
      expect(params.onFrame).toBeInstanceOf(Channel)
      expect(
        typeof (params.onFrame as Channel<ClashConnectionDetails_Serialize>)
          .onmessage,
      ).toBe('function')
      const id = nextId++
      channels.set(
        id,
        params.onFrame as Channel<ClashConnectionDetails_Serialize>,
      )
      return id
    }
    if (method === 'unsubscribe_clash_connection_details') {
      expect(Object.keys(params)).toEqual(['id'])
      unsubscribed.push(params.id as number)
      return null
    }
    throw new Error(`Unexpected RPC method: ${method}`)
  })

  const send = (id: number, frame: ClashConnectionDetails_Serialize) => {
    const channel = channels.get(id)!
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI_INTERNALS__.runCallback(channel.id, {
      index: index(channel),
      message: frame,
    })
  }

  return { channels, unsubscribed, send }
}

function Consumer({
  onRender,
  onState,
}: {
  onRender: (data: unknown) => void
  onState?: (state: ReturnType<typeof useClashConnectionDetails>) => void
}) {
  const state = useClashConnectionDetails()
  onRender(state.data)
  onState?.(state)
  return null
}

// Every test must unmount before `clearMocks()` runs: the provider's cleanup
// effect makes one last (async) unsubscribe `invoke` call, which throws if
// the mocked internals are already gone.
function teardown(
  onTestFinished: TestContext['onTestFinished'],
  screen: { unmount: () => Promise<void> },
  rpc: ReturnType<typeof createRpcClient>,
) {
  onTestFinished(async () => {
    await screen.unmount()
    await Promise.resolve()
    rpc.dispose()
    clearMocks()
  })
}

async function renderWithRpc(children: React.ReactNode) {
  const rpc = createRpcClient()
  const screen = await render(<RpcProvider rpc={rpc}>{children}</RpcProvider>)
  return { screen, rpc }
}

test('the first consumer subscribes, and the frame it renders came through the channel', async ({
  onTestFinished,
}) => {
  const { channels, send } = mockSubscriptions()
  const renders: unknown[] = []
  const states: ReturnType<typeof useClashConnectionDetails>[] = []

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer
        onRender={(data) => renders.push(data)}
        onState={(state) => states.push(state)}
      />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)

  await expect.poll(() => channels.size).toBe(1)
  expect(states.at(-1)?.status).toBe('connecting')
  expect(renders.at(-1)).toBe(null) // no frame yet

  send(0, details(1))
  await expect.poll(() => renders.at(-1)).toEqual(details(1))
  await expect.poll(() => states.at(-1)?.status).toBe('connected')
})

test('two consumers share a single subscription', async ({
  onTestFinished,
}) => {
  const { channels } = mockSubscriptions()

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)

  await expect.poll(() => channels.size).toBe(1)
  // No second subscription arrives once the first has settled.
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(channels.size).toBe(1)
})

test('unsubscribing on the last unmount ends the subscription', async ({
  onTestFinished,
}) => {
  const { channels, unsubscribed } = mockSubscriptions()

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)

  await expect.poll(() => channels.size).toBe(1)
  await screen.unmount()
  expect(unsubscribed).toEqual([0])
})

test('a consumer that unmounts before subscribe resolves still gets unsubscribed', async ({
  onTestFinished,
}) => {
  let resolveSubscribe!: (id: number) => void
  const channels = new Map<number, Channel<ClashConnectionDetails_Serialize>>()
  const unsubscribed: number[] = []

  mockRpcCalls((method, params) => {
    if (method === 'subscribe_clash_connection_details') {
      expect(Object.keys(params)).toEqual(['onFrame'])
      expect(params.onFrame).toBeInstanceOf(Channel)
      channels.set(
        0,
        params.onFrame as Channel<ClashConnectionDetails_Serialize>,
      )
      return new Promise<number>((resolve) => {
        resolveSubscribe = resolve
      })
    }
    if (method === 'unsubscribe_clash_connection_details') {
      expect(Object.keys(params)).toEqual(['id'])
      unsubscribed.push(params.id as number)
      return null
    }
    throw new Error(`Unexpected RPC method: ${method}`)
  })

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)

  await expect.poll(() => channels.size).toBe(1)
  // The consumer is gone before the subscribe call resolves.
  await screen.unmount()
  expect(unsubscribed).toEqual([])

  resolveSubscribe(0)
  await expect.poll(() => unsubscribed).toEqual([0])
})

test('a frozen boundary keeps the frame it had when freezing started', async ({
  onTestFinished,
}) => {
  const { channels, send } = mockSubscriptions()
  const renders: unknown[] = []
  const rpc = createRpcClient()
  const tree = (frozen: boolean) => (
    <RpcProvider rpc={rpc}>
      <ClashConnectionDetailsProvider connectorState="connected">
        <ClashConnectionDetailsFreezeBoundary frozen={frozen}>
          <Consumer onRender={(data) => renders.push(data)} />
        </ClashConnectionDetailsFreezeBoundary>
      </ClashConnectionDetailsProvider>
    </RpcProvider>
  )

  const screen = await render(tree(false))
  teardown(onTestFinished, screen, rpc)

  await expect.poll(() => channels.size).toBe(1)
  send(0, details(1))
  await expect.poll(() => renders.at(-1)).toEqual(details(1))

  // The render that starts freezing must not read the not-yet-captured frame.
  await screen.rerender(tree(true))
  send(0, details(2))
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(renders.at(-1)).toEqual(details(1))

  await screen.rerender(tree(false))
  await expect.poll(() => renders.at(-1)).toEqual(details(2))
})

test('a failed subscription reports the error and does not unsubscribe', async ({
  onTestFinished,
}) => {
  const error = new Error('subscription failed')
  const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  const states: ReturnType<typeof useClashConnectionDetails>[] = []
  mockRpcCalls((method, params) => {
    expect(method).toBe('subscribe_clash_connection_details')
    expect(Object.keys(params)).toEqual(['onFrame'])
    expect(params.onFrame).toBeInstanceOf(Channel)
    throw error
  })

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} onState={(state) => states.push(state)} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)
  onTestFinished(() => errorSpy.mockRestore())

  await expect
    .poll(() => errorSpy)
    .toHaveBeenCalledWith('failed to subscribe to connection details:', error)
  await expect.poll(() => states.at(-1)?.status).toBe('error')
})

test('a late rejected subscription from a replaced owner cannot overwrite a live retry', async ({
  onTestFinished,
}) => {
  const channels = new Map<number, Channel<ClashConnectionDetails_Serialize>>()
  const states: ReturnType<typeof useClashConnectionDetails>[] = []
  const index = nextIndex()
  let calls = 0
  let rejectFirst!: (error: Error) => void
  mockRpcCalls((method, params) => {
    if (method === 'subscribe_clash_connection_details') {
      const channel =
        params.onFrame as Channel<ClashConnectionDetails_Serialize>
      const id = calls++
      channels.set(id, channel)
      if (id === 0) {
        return new Promise<number>((_resolve, reject) => {
          rejectFirst = reject
        })
      }
      return id
    }
    if (method === 'unsubscribe_clash_connection_details') return null
    throw new Error(`Unexpected RPC method: ${method}`)
  })

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} onState={(state) => states.push(state)} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)
  await expect.poll(() => calls).toBe(1)

  states.at(-1)?.retry()
  await expect.poll(() => calls).toBe(2)
  const channel = channels.get(1)!
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(window as any).__TAURI_INTERNALS__.runCallback(channel.id, {
    index: index(channel),
    message: details(4),
  })
  await expect.poll(() => states.at(-1)?.status).toBe('connected')

  rejectFirst(new Error('late rejection'))
  await Promise.resolve()
  expect(states.at(-1)?.status).toBe('connected')
})

test('a rejected unsubscribe is reported without an unhandled rejection', async ({
  onTestFinished,
}) => {
  const error = new Error('unsubscribe failed')
  const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  let subscribed = false
  mockRpcCalls((method) => {
    if (method === 'subscribe_clash_connection_details') {
      subscribed = true
      return 0
    }
    if (method === 'unsubscribe_clash_connection_details') throw error
    throw new Error(`Unexpected RPC method: ${method}`)
  })

  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen, rpc)
  onTestFinished(() => errorSpy.mockRestore())

  await expect.poll(() => subscribed).toBe(true)
  await screen.unmount()
  await expect
    .poll(() => errorSpy)
    .toHaveBeenCalledWith(
      'failed to unsubscribe from connection details:',
      error,
    )
})

test('late frames from an unmounted provider do not reach a remounted provider', async ({
  onTestFinished,
}) => {
  const { channels, send } = mockSubscriptions()
  const renders: unknown[] = []
  const tree = () => (
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={(data) => renders.push(data)} />
    </ClashConnectionDetailsProvider>
  )

  const { screen: first, rpc: firstRpc } = await renderWithRpc(tree())
  await expect.poll(() => channels.size).toBe(1)
  await first.unmount()

  const { screen: second, rpc } = await renderWithRpc(tree())
  onTestFinished(async () => {
    await second.unmount()
    await Promise.resolve()
    rpc.dispose()
    firstRpc.dispose()
    clearMocks()
  })
  await expect.poll(() => channels.size).toBe(2)

  send(0, details(99))
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(renders.at(-1)).toBe(null)

  send(1, details(2))
  await expect.poll(() => renders.at(-1)).toEqual(details(2))
})

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  vi.stubGlobal('isTauri', true)
})
afterEach(() => vi.unstubAllGlobals())

test('browser consumers share an SSE stream and release it on unmount', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', false)
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  const streams: FakeSource[] = []
  class FakeSource {
    onopen: (() => void) | null = null
    onmessage: ((event: MessageEvent<string>) => void) | null = null
    onerror: ((event: Event) => void) | null = null
    close = vi.fn()
    constructor(readonly url: string) {
      streams.push(this)
    }
  }
  vi.stubGlobal('EventSource', FakeSource)
  const renders: unknown[] = []
  const states: ReturnType<typeof useClashConnectionDetails>[] = []
  const { screen, rpc } = await renderWithRpc(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer
        onRender={(data) => renders.push(data)}
        onState={(state) => states.push(state)}
      />
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  onTestFinished(async () => {
    await screen.unmount()
    await Promise.resolve()
    rpc.dispose()
  })
  await expect.poll(() => streams.length).toBe(1)
  expect(streams[0].url).toBe('/bridge/connection-details')
  streams[0].onerror?.(new Event('error'))
  await expect.poll(() => states.at(-1)?.status).toBe('error')
  streams[0].onopen?.()
  await expect.poll(() => states.at(-1)?.status).toBe('connecting')
  streams[0].onmessage?.(
    new MessageEvent('message', { data: JSON.stringify(details(3)) }),
  )
  await expect.poll(() => renders.at(-1)).toEqual(details(3))
  await expect.poll(() => states.at(-1)?.status).toBe('connected')
  await screen.unmount()
  expect(streams[0].close).toHaveBeenCalledOnce()
})
