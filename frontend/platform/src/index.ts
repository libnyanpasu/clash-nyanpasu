import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

export { isTauri }

export const isBrowser = () => typeof window !== 'undefined' && !isTauri()

type Platform =
  | 'aix'
  | 'android'
  | 'darwin'
  | 'freebsd'
  | 'haiku'
  | 'linux'
  | 'openbsd'
  | 'sunos'
  | 'win32'
  | 'cygwin'
  | 'netbsd'
  | 'unknown'

declare const OS_PLATFORM: Platform | undefined

export type System = 'macos' | 'windows' | 'linux' | 'unknown'

export function getSystem(userAgent: string, platform: string): System {
  if (userAgent.includes('Mac OS X') || platform === 'darwin') {
    return 'macos'
  }

  if (/win64|win32/i.test(userAgent) || platform === 'win32') {
    return 'windows'
  }

  if (/linux/i.test(userAgent) || platform === 'linux') {
    return 'linux'
  }

  return 'unknown'
}

export function getClientSystem(): System {
  const userAgent =
    typeof window === 'undefined' ? '' : (window.navigator?.userAgent ?? '')
  const platform = typeof OS_PLATFORM === 'undefined' ? 'unknown' : OS_PLATFORM

  return getSystem(userAgent, platform)
}

export const OS = getClientSystem()
export const isWindows = OS === 'windows'
export const isMacOS = OS === 'macos'
export const isLinux = OS === 'linux'

export type NativeTheme = 'light' | 'dark' | null

export interface WindowAdapter {
  isMaximized: () => Promise<boolean>
  isFullscreen: () => Promise<boolean>
  toggleMaximize: () => Promise<void>
  isMinimized: () => Promise<boolean>
  theme: () => Promise<NativeTheme>
  setTheme: (theme: NativeTheme) => Promise<void>
  onThemeChanged: (
    listener: (theme: NativeTheme) => void,
  ) => Promise<() => void>
}

export function createWindowAdapter(): WindowAdapter | null {
  if (!isTauri()) {
    return null
  }

  const appWindow = getCurrentWebviewWindow()

  return {
    isMaximized: () => appWindow.isMaximized(),
    isFullscreen: () => appWindow.isFullscreen(),
    toggleMaximize: () => appWindow.toggleMaximize(),
    isMinimized: () => appWindow.isMinimized(),
    theme: () => appWindow.theme(),
    setTheme: (theme) => appWindow.setTheme(theme),
    onThemeChanged: async (listener) =>
      appWindow.onThemeChanged((event) => listener(event.payload)),
  }
}
