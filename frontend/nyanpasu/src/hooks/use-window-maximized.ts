import { useCallback, useEffect } from 'react'
import { isMacOS } from '@/consts'
import { useSuspenseQuery } from '@tanstack/react-query'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = getCurrentWebviewWindow()

const IS_MAXIMIZED_QUERY_KEY = 'isMaximized'

export default function useWindowMaximized() {
  const query = useSuspenseQuery({
    queryKey: [IS_MAXIMIZED_QUERY_KEY],
    queryFn: async () => {
      // why maximized on macOS is fullscreen?
      if (isMacOS) {
        return await appWindow.isFullscreen()
      }

      return await appWindow.isMaximized()
    },
  })

  const handleToggleMaximize = useCallback(async () => {
    await appWindow.toggleMaximize()
    await query.refetch()
  }, [query])

  useEffect(() => {
    const onResize = () => {
      query.refetch()
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [query.refetch])

  return {
    isMaximized: query.data,
    toggleMaximize: handleToggleMaximize,
    ...query,
  }
}
