/**
 * Parsing of `install-config` deep links used to import a subscription profile.
 *
 * Supported shapes (percent-encoded `url`, optional `name`):
 *   clash://install-config?url=<http(s) url>&name=<display name>
 *   clash-nyanpasu://install-config?url=<http(s) url>
 *   clash:/install-config?url=...        (single-slash form on some platforms)
 */

export type InstallConfigDeepLink = {
  /** The http(s) subscription URL to import. */
  url: string
  /** Optional user-provided display name, `null` when absent. */
  name: string | null
}

const SUPPORTED_SCHEMES = ['clash:', 'clash-nyanpasu:']

const INSTALL_CONFIG_ACTION = 'install-config'

/**
 * Parse a raw deep link string into an install-config payload.
 * Returns `null` for any unsupported, malformed or non-install-config link.
 */
export function parseInstallConfigDeepLink(
  raw: string,
): InstallConfigDeepLink | null {
  let parsed: URL
  try {
    parsed = new URL(raw)
  } catch {
    return null
  }

  if (!SUPPORTED_SCHEMES.includes(parsed.protocol)) {
    return null
  }

  // Depending on platform and `scheme://` vs `scheme:/` form the action lands
  // either in the host or in the pathname.
  const action = (
    parsed.host || parsed.pathname.replace(/^\/+/, '')
  ).toLowerCase()
  if (action !== INSTALL_CONFIG_ACTION) {
    return null
  }

  // `URLSearchParams` already percent-decodes the values.
  const url = parsed.searchParams.get('url')
  if (!url || !isHttpUrl(url)) {
    return null
  }

  const name = parsed.searchParams.get('name')
  return { url, name: name && name.trim() ? name : null }
}

function isHttpUrl(value: string): boolean {
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}

/**
 * Where deep links wait: the backend keeps each one until a frontend takes it,
 * and pokes listeners whenever it queues another.
 */
export type DeepLinkQueue = {
  /** Calls `onPoke` for every queued link; resolves once it is listening. */
  listen: (onPoke: () => void) => Promise<() => void>
  /** Takes every queued link, oldest first. */
  take: () => Promise<string[]>
}

/**
 * Hands every queued deep link to `handle`, oldest first and one at a time,
 * until the returned function is called. The first drain waits for the
 * listener, so no link falls between the two: one queued before the listener
 * exists is in that drain, and one queued after it pokes another. Drains run
 * one at a time, and a poke during one runs exactly one more after it, so
 * batches reach `handle` in the order they were taken. A link queued twice
 * before a take is handled once, and so is a copy queued while that same link
 * was being handled. One reopened after it was handled is handled again,
 * unless a poke for another link arrived while it was being handled and the
 * reopen is queued before the next successful take: the mark lasts until
 * then, so this can happen while a later link of the same batch runs or after
 * a failed take. It follows from the poke carrying no URL.
 * When listening fails the queue is still drained once, so a link that is
 * already waiting is not stranded.
 */
export function receiveDeepLinks(
  queue: DeepLinkQueue,
  handle: (raw: string) => Promise<void>,
  onError: (error: unknown) => void,
): () => void {
  let disposed = false
  let unlisten: (() => void) | undefined
  let draining = false
  let drainAgain = false
  // The link being handled, and the links that were being handled when a poke
  // came in. The backend pokes right after it queues a link, so a copy that
  // poke announced is in the next take and is dropped from it.
  let current: string | undefined
  const inFlightCopies = new Set<string>()

  const drain = async () => {
    if (draining) {
      drainAgain = true
      return
    }
    draining = true
    try {
      do {
        drainAgain = false
        let links: string[]
        try {
          links = await queue.take()
        } catch (error) {
          onError(error)
          continue
        }
        const batch = new Set(links.filter((raw) => !inFlightCopies.has(raw)))
        inFlightCopies.clear()
        // A batch that was taken is handled whole, even by a caller that has
        // gone away meanwhile: nothing else will take those links again.
        for (const raw of batch) {
          current = raw
          try {
            await handle(raw)
          } catch (error) {
            onError(error)
          } finally {
            current = undefined
          }
        }
      } while (drainAgain && !disposed)
    } finally {
      draining = false
    }
  }

  queue
    .listen(() => {
      if (!disposed) {
        if (current !== undefined) {
          inFlightCopies.add(current)
        }
        drain().catch(onError)
      }
    })
    .then(
      (fn) => {
        // The caller may have gone away before `listen` resolved; the links
        // stay queued for the next listener.
        if (disposed) {
          fn()
          return
        }
        unlisten = fn
        drain().catch(onError)
      },
      (error) => {
        onError(error)
        if (!disposed) {
          drain().catch(onError)
        }
      },
    )

  return () => {
    disposed = true
    unlisten?.()
  }
}
