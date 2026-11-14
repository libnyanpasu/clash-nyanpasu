import { afterEach, expect, it, vi } from 'vitest'
import type { RuntimeInfo } from '@nyanpasu/platform'
import { getClientId } from '../src/client-id'
import { createConnectionDetailsTransport } from '../src/connection-details-transport'

class FakeEventSource {
  static instances: FakeEventSource[] = []
  readyState = 0
  onopen: (() => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  readonly close = vi.fn()

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this)
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  FakeEventSource.instances = []
})

const browser = {
  bridge: 'none',
  platform: { surface: 'browser' },
} as unknown as RuntimeInfo
const rpc = { subscribe: vi.fn(), unsubscribe: vi.fn() }

it('opens the browser stream with the tab client id and probes a closed stream', async () => {
  vi.stubGlobal('EventSource', FakeEventSource)
  const probeAuth = vi.fn().mockResolvedValue(undefined)
  const onError = vi.fn()
  const stop = createConnectionDetailsTransport(browser, rpc, {
    probeAuth,
  }).listen({ onOpen: vi.fn(), onMessage: vi.fn(), onError })
  const source = FakeEventSource.instances[0]

  expect(source.url).toBe(
    `/bridge/connection-details?client_id=${encodeURIComponent(getClientId())}`,
  )

  source.onerror?.(new Event('error'))
  expect(probeAuth).not.toHaveBeenCalled()

  source.readyState = 2
  source.onerror?.(new Event('error'))
  expect(probeAuth).toHaveBeenCalledOnce()
  expect(onError).toHaveBeenCalledTimes(2)
  stop()
})
