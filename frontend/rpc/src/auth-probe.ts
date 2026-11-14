const EVENT_SOURCE_CLOSED = 2

/** Asks the server whether the current credentials are still accepted. */
export type AuthProbe = () => Promise<unknown>

/**
 * `EventSource` hides the HTTP status of a rejected connection. A stream the
 * browser has given up on (`CLOSED`) is probed with a command instead, which
 * reports `401` through the command transport's `onUnauthorized`.
 */
export function createClosedStreamProbe(probe?: AuthProbe) {
  let inFlight = false
  return (source: Pick<EventSource, 'readyState'>) => {
    if (!probe || inFlight || source.readyState !== EVENT_SOURCE_CLOSED) return
    inFlight = true
    probe()
      .catch(() => {
        // Failures are classified by the command transport; nothing to add.
      })
      .finally(() => {
        inFlight = false
      })
  }
}
