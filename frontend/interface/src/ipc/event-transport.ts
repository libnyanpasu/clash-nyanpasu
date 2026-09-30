type EventCallback<T> = (event: {
  event: string
  id: number
  payload: T
}) => void

// A single connection avoids the browser's per-origin SSE connection limit.
// Active subscribers own its lifetime, including while it is connecting.
const listeners = new Map<string, Set<EventCallback<unknown>>>()
const resyncListeners = new Set<() => void>()
let source: EventSource | null = null
let sequence = 0

function connect() {
  if (source) return
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
  // Restore snapshots after initial attachment, every reconnect, and overflow.
  source.addEventListener('open', resync)
  source.addEventListener('resync', resync)
}

function disconnectIfIdle() {
  if (listeners.size || resyncListeners.size) return
  source?.close()
  source = null
  sequence = 0
}

export function listenHttpResync(callback: () => void): () => void {
  resyncListeners.add(callback)
  connect()
  return () => {
    resyncListeners.delete(callback)
    disconnectIfIdle()
  }
}

/** Cancellation is available even when the initial connection never opens. */
export function listenHttpEvent<T>(
  name: string,
  callback: EventCallback<T>,
): Promise<() => void> {
  let callbacks = listeners.get(name)
  if (!callbacks) {
    callbacks = new Set()
    listeners.set(name, callbacks)
  }
  const listener = callback as EventCallback<unknown>
  callbacks.add(listener)
  connect()
  let active = true
  return Promise.resolve(() => {
    if (!active) return
    active = false
    callbacks.delete(listener)
    if (!callbacks.size) listeners.delete(name)
    disconnectIfIdle()
  })
}

export function onceHttpEvent<T>(
  name: string,
  callback: EventCallback<T>,
): Promise<() => void> {
  let stop: (() => void) | undefined
  return listenHttpEvent<T>(name, (event) => {
    stop?.()
    callback(event)
  }).then((unlisten) => {
    stop = unlisten
    return unlisten
  })
}

export function emitHttpEvent<T>(name: string, _payload: T): Promise<void> {
  return Promise.reject(
    new Error(`Emitting ${name} is not available over HTTP`),
  )
}
