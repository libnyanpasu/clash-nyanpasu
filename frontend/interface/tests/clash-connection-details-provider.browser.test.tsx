import {
  afterEach,
  beforeEach,
  expect,
  test,
  vi,
  type TestContext,
} from 'vitest'
import { render } from 'vitest-browser-react'
import { Channel } from '@tauri-apps/api/core'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import type { ClashConnectionDetails_Serialize } from '../src/ipc/rpc-bindings'
import {
  ClashConnectionDetailsFreezeBoundary,
  ClashConnectionDetailsProvider,
  useClashConnectionDetails,
} from '../src/provider/clash-connection-details-provider'

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

function Consumer({ onRender }: { onRender: (data: unknown) => void }) {
  const { data } = useClashConnectionDetails()
  onRender(data)
  return null
}

// Every test must unmount before `clearMocks()` runs: the provider's cleanup
// effect makes one last (async) unsubscribe `invoke` call, which throws if
// the mocked internals are already gone.
function teardown(
  onTestFinished: TestContext['onTestFinished'],
  screen: { unmount: () => Promise<void> },
) {
  onTestFinished(async () => {
    await screen.unmount()
    clearMocks()
  })
}

test('the first consumer subscribes, and the frame it renders came through the channel', async ({
  onTestFinished,
}) => {
  const { channels, send } = mockSubscriptions()
  const renders: unknown[] = []

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={(data) => renders.push(data)} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen)

  await expect.poll(() => channels.size).toBe(1)
  expect(renders.at(-1)).toBe(null) // no frame yet

  send(0, details(1))
  await expect.poll(() => renders.at(-1)).toEqual(details(1))
})

test('two consumers share a single subscription', async ({
  onTestFinished,
}) => {
  const { channels } = mockSubscriptions()

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen)

  await expect.poll(() => channels.size).toBe(1)
  // No second subscription arrives once the first has settled.
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(channels.size).toBe(1)
})

test('unsubscribing on the last unmount ends the subscription', async ({
  onTestFinished,
}) => {
  const { channels, unsubscribed } = mockSubscriptions()

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen)

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

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  onTestFinished(() => clearMocks())

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
  const tree = (frozen: boolean) => (
    <ClashConnectionDetailsProvider connectorState="connected">
      <ClashConnectionDetailsFreezeBoundary frozen={frozen}>
        <Consumer onRender={(data) => renders.push(data)} />
      </ClashConnectionDetailsFreezeBoundary>
    </ClashConnectionDetailsProvider>
  )

  const screen = await render(tree(false))
  teardown(onTestFinished, screen)

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
  mockRpcCalls((method, params) => {
    expect(method).toBe('subscribe_clash_connection_details')
    expect(Object.keys(params)).toEqual(['onFrame'])
    expect(params.onFrame).toBeInstanceOf(Channel)
    throw error
  })

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen)
  onTestFinished(() => errorSpy.mockRestore())

  await expect
    .poll(() => errorSpy)
    .toHaveBeenCalledWith('failed to subscribe to connection details:', error)
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

  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  teardown(onTestFinished, screen)
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

  const first = await render(tree())
  await expect.poll(() => channels.size).toBe(1)
  await first.unmount()

  const second = await render(tree())
  teardown(onTestFinished, second)
  await expect.poll(() => channels.size).toBe(2)

  send(0, details(99))
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(renders.at(-1)).toBe(null)

  send(1, details(2))
  await expect.poll(() => renders.at(-1)).toEqual(details(2))
})

beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

test('browser consumers share an SSE stream and release it on unmount', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', false)
  const streams: FakeSource[] = []
  class FakeSource {
    onmessage: ((event: MessageEvent<string>) => void) | null = null
    close = vi.fn()
    constructor(readonly url: string) {
      streams.push(this)
    }
  }
  vi.stubGlobal('EventSource', FakeSource)
  const renders: unknown[] = []
  const screen = await render(
    <ClashConnectionDetailsProvider connectorState="connected">
      <Consumer onRender={(data) => renders.push(data)} />
      <Consumer onRender={() => {}} />
    </ClashConnectionDetailsProvider>,
  )
  onTestFinished(() => screen.unmount())
  await expect.poll(() => streams.length).toBe(1)
  expect(streams[0].url).toBe('/bridge/connection-details')
  streams[0].onmessage?.(
    new MessageEvent('message', { data: JSON.stringify(details(3)) }),
  )
  await expect.poll(() => renders.at(-1)).toEqual(details(3))
  await screen.unmount()
  expect(streams[0].close).toHaveBeenCalledOnce()
})
