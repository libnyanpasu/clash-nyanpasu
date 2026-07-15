import { useEffect, useRef } from 'react'
import { m } from '@/paraglide/messages'
import { parseInstallConfigDeepLink } from '@/utils/deep-link'
import { message } from '@/utils/notification'
import { commands, events, unwrapResult, useProfile } from '@nyanpasu/interface'
import { type UnlistenFn } from '@tauri-apps/api/event'

// Guard against duplicate registration across React StrictMode's double-mount
// and against multiple hook consumers: only one global listener should exist.
let listenerRegistered = false

/**
 * Imports the profile described by an `install-config` deep link. Two delivery
 * paths are handled: the `scheme-request-received` Tauri event (running app or
 * secondary instance) and the backend's pending deep link drained once on
 * startup (cold start, where the event may fire before the listener registers).
 * Mount once near the app root.
 */
export function useDeepLinkImport() {
  const { create } = useProfile()

  // Keep the latest mutation without re-registering the listener on every render.
  const createRef = useRef(create)
  createRef.current = create

  // Dedupe the same deep link arriving through both paths (event + pending
  // command may fire near-simultaneously on startup). Guarded synchronously
  // before the first await, so concurrent calls collapse to a single import.
  const inFlight = useRef(new Set<string>())

  useEffect(() => {
    if (listenerRegistered) {
      return
    }
    listenerRegistered = true

    let unlisten: UnlistenFn | undefined
    let disposed = false

    const handleDeepLink = async (raw: string) => {
      const parsed = parseInstallConfigDeepLink(raw)
      if (!parsed) {
        console.error('[deep-link] ignored unsupported deep link:', raw)
        await message(m.deep_link_import_invalid_message(), {
          title: m.deep_link_import_title(),
          kind: 'error',
        })
        return
      }

      if (inFlight.current.has(raw)) {
        return
      }
      inFlight.current.add(raw)

      try {
        await createRef.current.mutateAsync({
          type: 'url',
          data: { url: parsed.url, name: parsed.name, option: null },
        })

        await message(
          m.deep_link_import_success_message({
            name: parsed.name ?? parsed.url,
          }),
          { title: m.deep_link_import_title(), kind: 'info' },
        )
      } catch (error) {
        console.error('[deep-link] import failed:', error)
        await message(m.deep_link_import_failed_message(), {
          title: m.deep_link_import_title(),
          kind: 'error',
        })
      } finally {
        inFlight.current.delete(raw)
      }
    }

    events.schemeRequestReceivedEvent
      .listen(async (event) => {
        await handleDeepLink(event.payload.url)
      })
      .then((fn) => {
        // The effect may have been cleaned up before `listen` resolved.
        if (disposed) {
          fn()
          return
        }
        unlisten = fn
      })
      .catch((error) => {
        listenerRegistered = false
        console.error('[deep-link] failed to register listener:', error)
      })

    // Cold start: the event may have been emitted before the listener above was
    // registered, so drain the backend's pending deep link (take-and-clear) once.
    commands
      .getPendingDeepLink()
      .then(async (result) => {
        const pending = unwrapResult(result)
        if (pending) {
          await handleDeepLink(pending)
        }
      })
      .catch((error) => {
        console.error('[deep-link] failed to read pending deep link:', error)
      })

    return () => {
      disposed = true
      unlisten?.()
      listenerRegistered = false
    }
  }, [])
}
