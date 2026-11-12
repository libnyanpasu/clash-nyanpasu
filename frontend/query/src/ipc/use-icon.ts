import type { TrayIcon } from '@nyanpasu/rpc/types'
import { useRpc } from '../provider/rpc-provider'

export const useCachedIcon = (url: string | null) => {
  const rpc = useRpc()

  return url === null ? undefined : rpc.icons.url({ kind: 'cached', url })
}

export const useTrayIcon = (mode: TrayIcon, version?: number) => {
  const rpc = useRpc()

  return rpc.icons.url({ kind: 'tray', mode, version: version ?? null })
}

/** The statistic widget's own icon for a proxy state. */
export const useWidgetIcon = (mode: TrayIcon, version?: number) => {
  const rpc = useRpc()

  return rpc.icons.url({ kind: 'widget', mode, version: version ?? null })
}
