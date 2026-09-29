import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  emitHttpEvent,
  listenHttpEvent,
  onceHttpEvent,
} from '../src/ipc/event-transport'

const tauriEvent = vi.hoisted(() => ({
  emit: vi.fn(),
  listen: vi.fn(),
  once: vi.fn(),
}))
vi.mock('@tauri-apps/api/event', () => tauriEvent)

class FakeEventSource {
  static instances: FakeEventSource[] = []

  readonly listeners = new Map<string, EventListener>()
  readonly close = vi.fn()

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: EventListener) {
    this.listeners.set(type, listener)
  }

  removeEventListener(type: string, listener: EventListener) {
    if (this.listeners.get(type) === listener) this.listeners.delete(type)
  }

  dispatch(type: string, event: MessageEvent<string>) {
    this.listeners.get(type)?.(event)
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
  FakeEventSource.instances = []
  vi.resetModules()
})

describe('rpc event facade', () => {
  it('receives typed payloads from browser SSE and closes on unlisten', async () => {
    vi.stubGlobal('window', {})
    vi.stubGlobal('EventSource', FakeEventSource)
    const callback = vi.fn()

    const unlistenPromise = listenHttpEvent<{ sequence: number }>(
      'clash-ws-event',
      callback,
    )
    const source = FakeEventSource.instances[0]
    expect(source.url).toBe('/bridge/events?name=clash-ws-event')
    source.dispatch('open', new MessageEvent('open'))
    const unlisten = await unlistenPromise

    source.dispatch(
      'message',
      new MessageEvent('message', { data: JSON.stringify({ sequence: 42 }) }),
    )

    expect(callback).toHaveBeenCalledWith({
      event: 'clash-ws-event',
      id: 0,
      payload: { sequence: 42 },
    })
    unlisten()
    expect(source.listeners.has('message')).toBe(false)
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('rejects browser event emission explicitly', async () => {
    vi.stubGlobal('window', {})

    await expect(
      emitHttpEvent('window-ready-event', { label: 'main' }),
    ).rejects.toThrow(
      'Emitting window-ready-event is not available over experimental HTTP',
    )
    expect(tauriEvent.emit).not.toHaveBeenCalled()
  })

  it('closes a one-time browser subscription after the first event', async () => {
    vi.stubGlobal('window', {})
    vi.stubGlobal('EventSource', FakeEventSource)
    const callback = vi.fn()

    await onceHttpEvent('storage-value-changed-event', callback)
    const source = FakeEventSource.instances[0]
    source.dispatch(
      'message',
      new MessageEvent('message', { data: JSON.stringify({ key: 'web:x' }) }),
    )

    expect(callback).toHaveBeenCalledOnce()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('delegates subscriptions to Tauri in the desktop app', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const callback = vi.fn()
    const unlisten = vi.fn()
    tauriEvent.listen.mockResolvedValue(unlisten)

    const { rpc } = await import('../src/ipc/rpc')
    await expect(rpc.events.clashWsEvent.listen(callback)).resolves.toBe(
      unlisten,
    )
    expect(tauriEvent.listen).toHaveBeenCalledWith(
      'clash-ws-event',
      expect.any(Function),
    )
  })
})
