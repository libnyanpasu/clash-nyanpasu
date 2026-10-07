import { afterEach, describe, expect, it, vi } from 'vitest'
import { createHttpEventTransport } from '../src/event-transport'

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

afterEach(() => {
  vi.unstubAllGlobals()
  FakeEventSource.instances = []
})

function useBrowserEventSource() {
  vi.stubGlobal('EventSource', FakeEventSource)
}

describe('HTTP event transport', () => {
  it('multiplexes named subscriptions over one connection and closes when idle', async () => {
    useBrowserEventSource()
    const transport = createHttpEventTransport()
    const first = vi.fn()
    const second = vi.fn()
    const stopFirst = await transport.listen('clash-ws-event', first)
    const stopSecond = await transport.listen(
      'storage-value-changed-event',
      second,
    )

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
    transport.dispose()
  })

  it('routes one-shot and mutation subscriptions, then releases them on dispose', async () => {
    useBrowserEventSource()
    const transport = createHttpEventTransport()
    const once = vi.fn()
    const mutation = vi.fn()
    await transport.once('storage-value-changed-event', once)
    const stopMutation = await transport.listenMutation(mutation)
    const source = FakeEventSource.instances[0]

    source.dispatch(
      'message',
      JSON.stringify({ name: 'storage-value-changed-event', payload: 'x' }),
    )
    source.dispatch(
      'message',
      JSON.stringify({ name: 'storage-value-changed-event', payload: 'y' }),
    )
    source.dispatch(
      'message',
      JSON.stringify({
        name: 'nyanpasu://mutation',
        payload: { domain: 'profiles' },
      }),
    )
    expect(once).toHaveBeenCalledOnce()
    expect(mutation).toHaveBeenCalledWith({ domain: 'profiles' })

    transport.dispose()
    expect(source.close).toHaveBeenCalledOnce()
    source.dispatch(
      'message',
      JSON.stringify({
        name: 'nyanpasu://mutation',
        payload: { domain: 'state' },
      }),
    )
    expect(mutation).toHaveBeenCalledOnce()
    stopMutation()
    expect(source.close).toHaveBeenCalledOnce()
  })

  it('notifies resync listeners on stream open and explicit resync', () => {
    useBrowserEventSource()
    const transport = createHttpEventTransport()
    const resync = vi.fn()
    const stop = transport.listenResync(resync)
    const source = FakeEventSource.instances[0]

    source.dispatch('open')
    source.dispatch('error')
    source.dispatch('open')
    source.dispatch('resync', '{}')
    expect(resync).toHaveBeenCalledTimes(3)
    stop()
    expect(source.close).toHaveBeenCalledOnce()
    transport.dispose()
  })

  it('rejects event emission explicitly', async () => {
    useBrowserEventSource()
    const transport = createHttpEventTransport()
    await expect(
      transport.emit('window-message-event', { label: 'main' }),
    ).rejects.toThrow(
      'Emitting window-message-event is not available over HTTP',
    )
    transport.dispose()
  })
  it('keeps independent connections and listeners for two transport instances', async () => {
    useBrowserEventSource()
    const first = createHttpEventTransport()
    const second = createHttpEventTransport()
    const firstCallback = vi.fn()
    const secondCallback = vi.fn()
    await first.listen('clash-ws-event', firstCallback)
    const stopSecond = await second.listen('clash-ws-event', secondCallback)
    const [firstSource, secondSource] = FakeEventSource.instances
    expect(FakeEventSource.instances).toHaveLength(2)

    firstSource.dispatch(
      'message',
      JSON.stringify({ name: 'clash-ws-event', payload: 'first' }),
    )
    expect(firstCallback).toHaveBeenCalledOnce()
    expect(secondCallback).not.toHaveBeenCalled()
    first.dispose()
    expect(firstSource.close).toHaveBeenCalledOnce()
    expect(secondSource.close).not.toHaveBeenCalled()
    secondSource.dispatch(
      'message',
      JSON.stringify({ name: 'clash-ws-event', payload: 'second' }),
    )
    expect(secondCallback).toHaveBeenCalledOnce()
    stopSecond()
    second.dispose()
    expect(secondSource.close).toHaveBeenCalledOnce()
  })

  it('can cancel a subscription after connection failure before the first open', async () => {
    useBrowserEventSource()
    const transport = createHttpEventTransport()
    const stop = await transport.listen('clash-ws-event', vi.fn())
    const source = FakeEventSource.instances[0]
    source.dispatch('error')
    stop()
    expect(source.close).toHaveBeenCalledOnce()
    transport.dispose()
  })
})
