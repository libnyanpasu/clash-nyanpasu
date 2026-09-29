type EventCallback<T> = (event: {
  event: string
  id: number
  payload: T
}) => void

const eventPath = '/bridge/events'

/** Subscribe to a backend event through the experimental HTTP SSE bridge. */
export function listenHttpEvent<T>(
  name: string,
  callback: EventCallback<T>,
): Promise<() => void> {
  const source = new EventSource(
    `${eventPath}?name=${encodeURIComponent(name)}`,
  )
  let id = 0
  const onMessage = (message: MessageEvent<string>) => {
    let payload: T
    try {
      payload = JSON.parse(message.data) as T
    } catch (error) {
      console.error(`Failed to decode ${name} event`, error)
      return
    }
    callback({ event: name, id: id++, payload })
  }
  source.addEventListener('message', onMessage as EventListener)

  return new Promise((resolve) => {
    source.addEventListener(
      'open',
      () => {
        resolve(() => {
          source.removeEventListener('message', onMessage as EventListener)
          source.close()
        })
      },
      { once: true },
    )
  })
}

/** Subscribe to one event through the experimental HTTP SSE bridge. */
export function onceHttpEvent<T>(
  name: string,
  callback: EventCallback<T>,
): Promise<() => void> {
  const source = new EventSource(
    `${eventPath}?name=${encodeURIComponent(name)}`,
  )
  const onMessage = (message: MessageEvent<string>) => {
    let payload: T
    try {
      payload = JSON.parse(message.data) as T
    } catch (error) {
      console.error(`Failed to decode ${name} event`, error)
      return
    }
    stop()
    callback({ event: name, id: 0, payload })
  }
  const stop = () => {
    source.removeEventListener('message', onMessage as EventListener)
    source.close()
  }
  source.addEventListener('message', onMessage as EventListener)
  return Promise.resolve(stop)
}

/** Emit a frontend event through Tauri; the experimental HTTP bridge is receive-only. */
export function emitHttpEvent<T>(name: string, payload: T): Promise<void> {
  return Promise.reject(
    new Error(`Emitting ${name} is not available over experimental HTTP`),
  )
}
