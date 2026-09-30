import { expect, test, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { Channel } from '@tauri-apps/api/core'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import type { ClashConnectionDetails_Serialize } from '../src/ipc/bindings'
import {
  ClashConnectionDetailsFreezeBoundary,
  ClashConnectionDetailsProvider,
  useClashConnectionDetails,
} from '../src/provider/clash-connection-details-provider'

const details = (sequence: number): ClashConnectionDetails_Serialize => ({
  sessionId: 'session',
  revision: String(sequence),
  freshness: 'Fresh',
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

// Resolves subscribe calls with sequential ids and captures each channel, so
// a test can push a frame or assert a subscription ended.
function mockSubscriptions() {
  let nextId = 0
  const channels = new Map<number, Channel<ClashConnectionDetails_Serialize>>()
  const unsubscribed: number[] = []
  const index = nextIndex()

  mockIPC((cmd, args) => {
    if (cmd === 'subscribe_clash_connection_details') {
      const id = nextId++
      channels.set(
        id,
        (args as { onFrame: Channel<ClashConnectionDetails_Serialize> })
          .onFrame,
      )
      return id
    }
    if (cmd === 'unsubscribe_traffic_subscription') {
      unsubscribed.push((args as { id: number }).id)
      return null
    }
    throw new Error(`Unexpected IPC command: ${cmd}`)
  })

  const send = (id: number, frame: ClashConnectionDetails_Serialize | null) => {
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

test('session revisions preserve large integers and retired streams clear details', async ({
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
  const current = { ...details(1), revision: '9007199254740993' }
  send(0, current)
  await expect.poll(() => renders.at(-1)).toEqual(current)
  send(0, { ...current, revision: '9007199254740992' })
  const next = { ...current, revision: '9007199254740994' }
  send(0, next)
  await expect.poll(() => renders.at(-1)).toEqual(next)
  send(0, null)
  await expect.poll(() => renders.at(-1)).toBe(null)
  const replacement = { ...details(1), sessionId: 'replacement' }
  send(0, replacement)
  await expect.poll(() => renders.at(-1)).toEqual(replacement)
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

  mockIPC((cmd, args) => {
    if (cmd === 'subscribe_clash_connection_details') {
      channels.set(
        0,
        (args as { onFrame: Channel<ClashConnectionDetails_Serialize> })
          .onFrame,
      )
      return new Promise<number>((resolve) => {
        resolveSubscribe = resolve
      })
    }
    if (cmd === 'unsubscribe_traffic_subscription') {
      unsubscribed.push((args as { id: number }).id)
      return null
    }
    throw new Error(`Unexpected IPC command: ${cmd}`)
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
