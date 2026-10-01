import { useCallback, useEffect } from 'react'
import { isMacOS } from '@/consts'
import { useSuspenseQuery } from '@tanstack/react-query'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = isTauri() ? getCurrentWebviewWindow() : null

const IS_MAXIMIZED_QUERY_KEY = 'isMaximized'

export default function useWindowMaximized() {
  // Only `data` is read, so the tracked query re-renders callers when the
  // state changes, not on every fetch.
  const { data: isMaximized, refetch } = useSuspenseQuery({
    queryKey: [IS_MAXIMIZED_QUERY_KEY],
    queryFn: async () => {
      if (!appWindow) return false
      // why maximized on macOS is fullscreen?
      if (isMacOS) {
        return await appWindow?.isFullscreen()
      }

      return await appWindow?.isMaximized()
    },
  })

  const handleToggleMaximize = useCallback(async () => {
    await appWindow?.toggleMaximize()
    await refetch()
  }, [refetch])

  useEffect(() => {
    // Resizing fires an event every frame; ask over IPC once it settles, and
    // share the request with the other callers of this hook.
    let timer: ReturnType<typeof setTimeout> | undefined

    const onResize = () => {
      clearTimeout(timer)
      timer = setTimeout(() => refetch({ cancelRefetch: false }), 100)
    }

    window.addEventListener('resize', onResize)

    return () => {
      clearTimeout(timer)
      window.removeEventListener('resize', onResize)
    }
  }, [refetch])

  return {
    isMaximized,
    toggleMaximize: handleToggleMaximize,
  }
}
