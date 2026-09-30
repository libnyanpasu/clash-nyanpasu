import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  emitHttpEvent,
  listenHttpEvent,
  listenHttpResync,
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
  dispatch(type: string, data = '') {
    this.listeners.get(type)?.(new MessageEvent(type, { data }))
  }
}
const cleanup: (() => void)[] = []
afterEach(() => {
  cleanup.splice(0).forEach((stop) => stop())
  vi.unstubAllGlobals()
  vi.clearAllMocks()
  FakeEventSource.instances = []
  vi.resetModules()
})
function browser() {
  vi.stubGlobal('window', {})
  vi.stubGlobal('EventSource', FakeEventSource)
}

describe('rpc event facade', () => {
  it('multiplexes different event subscriptions over one connection', async () => {
    browser()
    const first = vi.fn(),
      second = vi.fn()
    const stopFirst = await listenHttpEvent('clash-ws-event', first)
    const stopSecond = await listenHttpEvent(
      'storage-value-changed-event',
      second,
    )
    cleanup.push(stopFirst, stopSecond)
    expect(FakeEventSource.instances).toHaveLength(1)
    const source = FakeEventSource.instances[0]
    expect(source.url).toBe('/bridge/events')
    source.dispatch(
      'message',
      JSON.stringify({ name: 'clash-ws-event', payload: { sequence: 42 } }),
    )
    expect(first).toHaveBeenCalledWith({
      event: 'clash-ws-event',
      id: 0,
      payload: { sequence: 42 },
    })
    expect(second).not.toHaveBeenCalled()
    stopFirst()
    expect(source.close).not.toHaveBeenCalled()
    source.dispatch(
      'message',
      JSON.stringify({
        name: 'storage-value-changed-event',
        payload: { key: 'web:x' },
      }),
    )
    expect(second).toHaveBeenCalledOnce()
    stopSecond()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('can cancel before the first connection opens or after it fails', async () => {
    browser()
    const stop = await listenHttpEvent('clash-ws-event', vi.fn())
    cleanup.push(stop)
    const source = FakeEventSource.instances[0]
    source.dispatch('error')
    stop()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('requests snapshot recovery on initial open, reconnect and overflow', () => {
    browser()
    const callback = vi.fn()
    const stop = listenHttpResync(callback)
    cleanup.push(stop)
    const source = FakeEventSource.instances[0]
    source.dispatch('open')
    source.dispatch('error')
    source.dispatch('open')
    source.dispatch('resync', '{}')
    expect(callback).toHaveBeenCalledTimes(3)
    stop()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('releases a one-time listener after its first matching event', async () => {
    browser()
    const callback = vi.fn()
    const stop = await onceHttpEvent('storage-value-changed-event', callback)
    cleanup.push(stop)
    const source = FakeEventSource.instances[0]
    source.dispatch(
      'message',
      JSON.stringify({
        name: 'storage-value-changed-event',
        payload: { key: 'web:x' },
      }),
    )
    source.dispatch(
      'message',
      JSON.stringify({
        name: 'storage-value-changed-event',
        payload: { key: 'web:y' },
      }),
    )
    expect(callback).toHaveBeenCalledOnce()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('rejects browser event emission explicitly', async () => {
    browser()
    await expect(
      emitHttpEvent('window-ready-event', { label: 'main' }),
    ).rejects.toThrow('Emitting window-ready-event is not available over HTTP')
  })

  it('delegates desktop events to Tauri', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const callback = vi.fn(),
      unlisten = vi.fn()
    tauriEvent.listen.mockResolvedValue(unlisten)
    const { rpc } = await import('../src/ipc/rpc')
    await expect(rpc.events.clashWsEvent.listen(callback)).resolves.toBe(
      unlisten,
    )
    expect(tauriEvent.listen).toHaveBeenCalledWith(
      'clash-ws-event',
      expect.any(Function),
    )
    rpc.listenResync(callback)()
    expect(callback).not.toHaveBeenCalled()
  })
})
