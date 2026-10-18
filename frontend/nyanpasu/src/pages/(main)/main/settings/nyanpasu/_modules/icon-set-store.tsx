import { createContext, useContext, type ComponentType } from 'react'
import type { LazyImageProps } from '@nyanpasu/ui/lazy-image'
import { TrayImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { mutations, queries } from '@/services/rpc'
import { isWindows } from '@nyanpasu/platform'
import { useTrayIcon } from '@nyanpasu/query'
import type { TrayIcon } from '@nyanpasu/rpc/types'

/**
 * Where an icon set with a normal, a TUN and a system proxy icon is stored:
 * the tray's, or the statistic widget's own. The icon set editor reads and
 * writes through it, so one editor serves both.
 */
export interface IconSetStore {
  useIcon: (mode: TrayIcon) => { data?: { data_url: string } }
  Image: ComponentType<
    Omit<LazyImageProps, 'src'> & { mode: TrayIcon; version?: number }
  >
  isSetQuery: typeof queries.isTrayIconSet
  setIcon: typeof mutations.setTrayIcon
  setIconFromBytes: typeof mutations.setTrayIconFromBytes
  /** Query key prefixes of the icon and is-set queries, for invalidation. */
  iconKey: string
  isSetKey: string
  /** Whether the preset cards are offered; otherwise each state is edited on its own. */
  presets: boolean
  messages: {
    label: () => string
    presetTitle: () => string
    setSuccess: () => string
    setFailed: () => string
  }
}

export const TRAY_ICON_STORE: IconSetStore = {
  useIcon: (mode) => useTrayIcon(mode),
  Image: TrayImage,
  isSetQuery: (mode) => queries.isTrayIconSet(mode),
  setIcon: mutations.setTrayIcon,
  setIconFromBytes: mutations.setTrayIconFromBytes,
  iconKey: 'getTrayIcon',
  isSetKey: 'isTrayIconSet',
  // Presets are Windows-only; other platforms keep the per-state editor.
  presets: isWindows,
  messages: {
    label: m.settings_nyanpasu_tray_icon,
    presetTitle: m.settings_nyanpasu_tray_icon_preset_title,
    setSuccess: m.settings_nyanpasu_tray_icon_set_success,
    setFailed: m.settings_nyanpasu_tray_icon_set_failed,
  },
}

const IconSetStoreContext = createContext<IconSetStore>(TRAY_ICON_STORE)

export const IconSetStoreProvider = IconSetStoreContext.Provider

export const useIconSetStore = () => useContext(IconSetStoreContext)
