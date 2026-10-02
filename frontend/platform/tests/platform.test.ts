import { beforeEach, describe, expect, test, vi } from 'vitest'
import { createWindowAdapter, getSystem } from '../src/index.ts'

const mocks = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getCurrentWebviewWindow: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: mocks.isTauri,
}))

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: mocks.getCurrentWebviewWindow,
}))

describe('getSystem', () => {
  test('recognizes operating systems from browser agents and build platforms', () => {
    const cases = [
      ['Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', 'unknown', 'macos'],
      ['', 'darwin', 'macos'],
      ['Mozilla/5.0 (Windows NT 10.0; Win64; x64)', 'unknown', 'windows'],
      ['', 'win32', 'windows'],
      ['Mozilla/5.0 (X11; Linux x86_64)', 'unknown', 'linux'],
      ['', 'linux', 'linux'],
      ['', 'freebsd', 'unknown'],
    ] as const

    for (const [userAgent, platform, expected] of cases) {
      expect(getSystem(userAgent, platform)).toBe(expected)
    }
  })
})

describe('createWindowAdapter', () => {
  beforeEach(() => {
    mocks.isTauri.mockReset()
    mocks.getCurrentWebviewWindow.mockReset()
  })

  test('does not request a native window in a browser', () => {
    mocks.isTauri.mockReturnValue(false)

    expect(createWindowAdapter()).toBeNull()
    expect(mocks.getCurrentWebviewWindow).not.toHaveBeenCalled()
  })

  test('creates the native window adapter only after Tauri is detected', async () => {
    const nativeWindow = {
      isMaximized: vi.fn().mockResolvedValue(true),
      isFullscreen: vi.fn().mockResolvedValue(false),
      toggleMaximize: vi.fn().mockResolvedValue(undefined),
      isMinimized: vi.fn().mockResolvedValue(false),
      theme: vi.fn().mockResolvedValue('dark'),
      setTheme: vi.fn().mockResolvedValue(undefined),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    }
    mocks.isTauri.mockReturnValue(true)
    mocks.getCurrentWebviewWindow.mockReturnValue(nativeWindow)

    const adapter = createWindowAdapter()

    expect(mocks.getCurrentWebviewWindow).toHaveBeenCalledTimes(1)
    await expect(adapter?.isMaximized()).resolves.toBe(true)
    await adapter?.setTheme('light')
    expect(nativeWindow.setTheme).toHaveBeenCalledWith('light')
  })
})
