import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

export type Surface = 'desktop' | 'browser' | 'unknown'
export type ClientOs = 'windows' | 'macos' | 'linux' | 'unknown'
export type Bridge = 'tauri' | 'none'
export type PlatformInfo = Readonly<{ surface: Surface; clientOs: ClientOs }>
export type RuntimeInfo = Readonly<{ platform: PlatformInfo; bridge: Bridge }>
export type PlatformKey = 'desktop' | 'browser' | Exclude<ClientOs, 'unknown'>
export type PlatformSelector =
  | PlatformKey
  | { surface: Surface; clientOs?: ClientOs }
  | { surface?: Surface; clientOs: ClientOs }
export type PlatformCondition =
  PlatformSelector | readonly [PlatformSelector, ...PlatformSelector[]]

export function matchesPlatform(
  platform: PlatformInfo,
  when: PlatformCondition,
): boolean {
  if (Array.isArray(when)) {
    return when.some((condition) => matchesPlatform(platform, condition))
  }
  if (typeof when === 'string') {
    if (when === 'desktop' || when === 'browser')
      return platform.surface === when
    return platform.clientOs === when && platform.surface === 'desktop'
  }
  const selector = when as { surface?: Surface; clientOs?: ClientOs }
  return (
    (selector.surface === undefined || platform.surface === selector.surface) &&
    (selector.clientOs === undefined || platform.clientOs === selector.clientOs)
  )
}

type MatchCases<T> = {
  cases: readonly { when: PlatformCondition; run: () => T }[]
  default?: () => T
}
type MatchMap<T> = Partial<Record<PlatformKey, () => T>> & { default?: () => T }
export function matchPlatform<T>(
  platform: PlatformInfo,
  options: MatchCases<T> | MatchMap<T>,
): T | undefined {
  if ('cases' in options) {
    const found = options.cases.find(({ when }) =>
      matchesPlatform(platform, when),
    )
    return found ? found.run() : options.default?.()
  }
  const osKey =
    platform.surface === 'desktop' && platform.clientOs !== 'unknown'
      ? platform.clientOs
      : undefined
  const selected =
    (osKey && options[osKey]) || options[platform.surface as PlatformKey]
  return selected ? selected() : options.default?.()
}

export function getClientOs(userAgent: string): ClientOs {
  if (/android|iphone|ipad|mobile/i.test(userAgent)) return 'unknown'
  if (/windows/i.test(userAgent)) return 'windows'
  if (/macintosh|mac os x/i.test(userAgent)) return 'macos'
  if (/linux|x11/i.test(userAgent)) return 'linux'
  return 'unknown'
}

export function detectRuntime(): RuntimeInfo {
  if (typeof window === 'undefined') {
    return {
      platform: { surface: 'unknown', clientOs: 'unknown' },
      bridge: 'none',
    }
  }
  const bridge: Bridge = isTauri() ? 'tauri' : 'none'
  return {
    platform: {
      surface: bridge === 'tauri' ? 'desktop' : 'browser',
      clientOs: getClientOs(window.navigator?.userAgent ?? ''),
    },
    bridge,
  }
}

export interface BrowserAdapter {
  alert: (message: string) => void
  confirm: (message: string) => boolean
  navigate: (url: string) => void
  openExternal: (url: string) => void
  download: (url: string) => void
}

export function createBrowserAdapter(
  surface: Surface = detectRuntime().platform.surface,
): BrowserAdapter | null {
  if (surface !== 'browser' || typeof window === 'undefined') return null
  return {
    alert: (message) => window.alert(message),
    confirm: (message) => window.confirm(message),
    navigate: (url) => window.location.assign(url),
    openExternal: (url) => window.open(url, '_blank', 'noopener,noreferrer'),
    download: (url) => {
      const anchor = window.document.createElement('a')
      anchor.href = url
      anchor.click()
    },
  }
}

export type NativeTheme = 'light' | 'dark' | null
export interface WindowAdapter {
  label: () => string
  close: () => Promise<void>
  show: () => Promise<void>
  minimize: () => Promise<void>
  maximize: () => Promise<void>
  setAlwaysOnTop: (value: boolean) => Promise<void>
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

export function createWindowAdapter(
  bridge: Bridge = detectRuntime().bridge,
): WindowAdapter | null {
  if (bridge !== 'tauri') return null
  const appWindow = getCurrentWebviewWindow()
  return {
    label: () => appWindow.label,
    close: () => appWindow.close(),
    show: () => appWindow.show(),
    minimize: () => appWindow.minimize(),
    maximize: () => appWindow.maximize(),
    setAlwaysOnTop: (value) => appWindow.setAlwaysOnTop(value),
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

export type WebNotificationOptions = { title: string; body?: string }
export async function showWebNotification({
  title,
  body,
}: WebNotificationOptions): Promise<void> {
  if (typeof window === 'undefined') return
  const fallback = () => window.alert(body ? `${title}: ${body}` : title)
  if (!('Notification' in window)) return fallback()
  try {
    const permission =
      Notification.permission === 'default'
        ? await Notification.requestPermission()
        : Notification.permission
    if (permission === 'granted') {
      const notification = new Notification(title, { body })
      notification.addEventListener('click', () => window.focus(), {
        once: true,
      })
      return
    }
  } catch {
    // Fall back when browser notifications are unavailable.
  }
  fallback()
}
