export type RpcEvent<T> = {
  event: string
  id: number
  payload: T
}

export type RpcEventCallback<T> = (event: RpcEvent<T>) => void

export interface RpcEventTransport {
  listen<T>(name: string, callback: RpcEventCallback<T>): Promise<() => void>
  once<T>(name: string, callback: RpcEventCallback<T>): Promise<() => void>
  emit<T>(name: string, payload: T): Promise<void>
  listenMutation(callback: (payload: unknown) => void): Promise<() => void>
  listenResync(callback: () => void): () => void
  dispose(): void
}

const MUTATION_EVENT_NAME = 'nyanpasu://mutation'

/** Each client owns one shared SSE connection, released after its last listener. */
export function createHttpEventTransport(): RpcEventTransport {
  const listeners = new Map<string, Set<RpcEventCallback<unknown>>>()
  const resyncListeners = new Set<() => void>()
  let source: EventSource | null = null
  let sequence = 0
  let disposed = false

  const connect = () => {
    if (source || disposed) return
    source = new EventSource('/bridge/events')
    source.addEventListener('message', (message) => {
      let event: { name: string; payload: unknown }
      try {
        event = JSON.parse((message as MessageEvent<string>).data)
      } catch (error) {
        console.error('Failed to decode backend event', error)
        return
      }
      const callbacks = listeners.get(event.name)
      const id = sequence++
      for (const callback of Array.from(callbacks ?? [])) {
        callback({ event: event.name, id, payload: event.payload })
      }
    })
    const resync = () => {
      for (const callback of resyncListeners) callback()
    }
    source.addEventListener('open', resync)
    source.addEventListener('resync', resync)
  }

  const disconnectIfIdle = () => {
    if (listeners.size || resyncListeners.size) return
    source?.close()
    source = null
    sequence = 0
  }

  const listen = <T>(
    name: string,
    callback: RpcEventCallback<T>,
  ): Promise<() => void> => {
    if (disposed) return Promise.resolve(() => {})
    let callbacks = listeners.get(name)
    if (!callbacks) {
      callbacks = new Set()
      listeners.set(name, callbacks)
    }
    const listener = callback as RpcEventCallback<unknown>
    callbacks.add(listener)
    connect()
    let active = true
    return Promise.resolve(() => {
      if (!active) return
      active = false
      callbacks?.delete(listener)
      if (!callbacks?.size) listeners.delete(name)
      disconnectIfIdle()
    })
  }

  return {
    listen,
    once: <T>(name: string, callback: RpcEventCallback<T>) => {
      let stop: (() => void) | undefined
      return listen<T>(name, (event) => {
        stop?.()
        callback(event)
      }).then((unlisten) => {
        stop = unlisten
        return unlisten
      })
    },
    emit: (name: string) =>
      Promise.reject(new Error(`Emitting ${name} is not available over HTTP`)),
    listenMutation: (callback) =>
      listen(MUTATION_EVENT_NAME, ({ payload }) => callback(payload)),
    listenResync: (callback) => {
      if (disposed) return () => {}
      resyncListeners.add(callback)
      connect()
      return () => {
        resyncListeners.delete(callback)
        disconnectIfIdle()
      }
    },
    dispose: () => {
      if (disposed) return
      disposed = true
      source?.close()
      source = null
      listeners.clear()
      resyncListeners.clear()
      sequence = 0
    },
  }
}
