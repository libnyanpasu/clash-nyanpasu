import { useEffect, useRef } from 'react'
import { m } from '@/paraglide/messages'
import { parseInstallConfigDeepLink, receiveDeepLinks } from '@/utils/deep-link'
import { message } from '@/utils/notification'
import { commands, events, unwrapResult, useProfile } from '@nyanpasu/interface'

// Guard against duplicate registration across React StrictMode's double-mount
// and against multiple hook consumers: only one global listener should exist.
let listenerRegistered = false

/**
 * Imports the profile described by each `install-config` deep link, one at a
 * time. The backend queues every link until a frontend takes it and pokes
 * listeners through the `scheme-request-received` Tauri event, so a link that
 * arrives before this hook listens, or while the webview reloads, is taken
 * once it does. Because links are handled in turn, an import that hangs or a
 * result dialog left open holds back later links, which stay queued in the
 * backend meanwhile. Mount once, in the main window.
 */
export function useDeepLinkImport() {
  const { create } = useProfile()

  // Keep the latest mutation without re-registering the listener on every render.
  const createRef = useRef(create)
  createRef.current = create

  useEffect(() => {
    if (listenerRegistered) {
      return
    }
    listenerRegistered = true

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
          error,
        })
      }
    }

    const stop = receiveDeepLinks(
      {
        listen: (onPoke) =>
          events.schemeRequestReceivedEvent.listen(onPoke).catch((error) => {
            listenerRegistered = false
            throw error
          }),
        take: async () => unwrapResult(await commands.takePendingDeepLinks()),
      },
      handleDeepLink,
      (error) => {
        console.error('[deep-link] delivery failed:', error)
      },
    )

    return () => {
      stop()
      listenerRegistered = false
    }
  }, [])
}
