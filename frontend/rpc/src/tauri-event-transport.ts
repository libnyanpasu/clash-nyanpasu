import { emit, listen, type Event } from '@tauri-apps/api/event'
import type { RpcEventTransport } from './event-transport'

export function createTauriEventTransport(): RpcEventTransport {
  const unlisteners = new Set<() => void>()
  let disposed = false
  let sequence = 0

  const listenEvent = <T>(
    name: string,
    callback: (event: { event: string; id: number; payload: T }) => void,
    oneShot: boolean,
  ) => {
    if (disposed) return Promise.resolve(() => {})
    let active = true
    let stop: (() => void) | undefined
    const handler = (event: Event<T>) => {
      if (!active || disposed) return
      if (oneShot) release()
      callback({
        event: event.event,
        id: event.id ?? sequence++,
        payload: event.payload,
      })
    }
    const release = () => {
      if (!active) return
      active = false
      if (stop) {
        stop()
        unlisteners.delete(stop)
      }
    }
    const registration = listen(name, handler)
    return registration.then((unlisten) => {
      if (disposed || !active) {
        unlisten()
        return release
      }
      stop = unlisten
      unlisteners.add(unlisten)
      return release
    })
  }

  return {
    listen: (name, callback) => listenEvent(name, callback, false),
    once: (name, callback) => listenEvent(name, callback, true),
    emit: (name, payload) => emit(name, payload),
    listenMutation: (callback) =>
      listenEvent(
        'nyanpasu://mutation',
        ({ payload }) => callback(payload),
        false,
      ),
    listenResync: () => () => {},
    dispose: () => {
      if (disposed) return
      disposed = true
      for (const unlisten of unlisteners) unlisten()
      unlisteners.clear()
    },
  }
}
