import { afterEach, expect, it, vi } from 'vitest'
import type { RuntimeInfo } from '@nyanpasu/platform'
import { createDefaultRpcTransport } from '../src'

class FakeEventSource {
  static instances: FakeEventSource[] = []
  readyState = 0
  readonly listeners = new Map<string, () => void>()
  readonly close = vi.fn()

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: () => void) {
    this.listeners.set(type, listener)
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  FakeEventSource.instances = []
})

it('reports a revoked session discovered through a closed event stream', async () => {
  vi.stubGlobal('EventSource', FakeEventSource)
  const fetchMock = vi.fn().mockResolvedValue({
    ok: false,
    status: 401,
    json: async () => ({ kind: 'forbidden', message: 'expired' }),
  })
  vi.stubGlobal('fetch', fetchMock)
  const onUnauthorized = vi.fn()
  const runtime = {
    bridge: 'none',
    platform: { surface: 'browser' },
  } as unknown as RuntimeInfo
  const transport = createDefaultRpcTransport(runtime, { onUnauthorized })

  transport.events.listenResync(() => {})
  const source = FakeEventSource.instances[0]
  source.readyState = 2
  source.listeners.get('error')?.()

  await vi.waitFor(() => expect(onUnauthorized).toHaveBeenCalledOnce())
  expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({
    method: 'rpc_describe',
    params: {},
  })
  transport.events.dispose()
})
