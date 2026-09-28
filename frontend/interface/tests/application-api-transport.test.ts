import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createHttpApplicationApi,
  createTauriApplicationApi,
} from '../src/application-api'

const channels: Array<{ onmessage?: (event: unknown) => void }> = []
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage?: (event: unknown) => void
    constructor() {
      channels.push(this)
    }
  },
}))

beforeEach(() => {
  vi.stubGlobal('window', {})
})

afterEach(() => {
  channels.length = 0
  vi.unstubAllGlobals()
})

describe('application API transports', () => {
  it('routes typed Tauri calls and closes Channel subscriptions', async () => {
    const call = vi.fn().mockResolvedValue({ current: null, items: [] })
    const subscribe = vi.fn().mockResolvedValue(17)
    const unsubscribe = vi.fn().mockResolvedValue(true)
    window.__NYANPASU_API__ = { call, subscribe, unsubscribe }

    const api = createTauriApplicationApi()
    await expect(api.call('profiles.list', {})).resolves.toEqual({
      current: null,
      items: [],
    })
    expect(call).toHaveBeenCalledWith('profiles.list', {})

    const events: unknown[] = []
    const close = await api.subscribe('clash.events', null, (event) =>
      events.push(event),
    )
    const event = {
      sequence: 3,
      update: { kind: 'history_cleared', data: 'logs' },
    }
    channels[0].onmessage?.(event)
    expect(events).toEqual([event])
    expect(subscribe).toHaveBeenCalledWith(
      'clash.events',
      null,
      expect.anything(),
    )

    await Promise.all([close(), close()])
    expect(unsubscribe).toHaveBeenCalledTimes(1)
    expect(unsubscribe).toHaveBeenCalledWith(17)
  })

  it('sends fn_name and params through the HTTP adapter', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ current: null, items: [] }),
    })
    vi.stubGlobal('fetch', fetchMock)

    const api = createHttpApplicationApi('http://127.0.0.1:8080/')
    await api.call('profiles.list', {})
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:8080/call', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ fn_name: 'profiles.list', params: {} }),
    })
  })
})
