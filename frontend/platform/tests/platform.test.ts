import { beforeEach, describe, expect, test, vi } from 'vitest'
import {
  createWindowAdapter,
  getClientOs,
  matchesPlatform,
  matchPlatform,
} from '../src/index.ts'

const mocks = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getCurrentWebviewWindow: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ isTauri: mocks.isTauri }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: mocks.getCurrentWebviewWindow,
}))

describe('platform matching', () => {
  const desktopWindows = { surface: 'desktop', clientOs: 'windows' } as const
  test('keeps browser OS hints separate from desktop identity', () => {
    expect(
      matchesPlatform({ surface: 'browser', clientOs: 'windows' }, 'windows'),
    ).toBe(false)
    expect(
      matchesPlatform(
        { surface: 'browser', clientOs: 'windows' },
        { surface: 'browser', clientOs: 'windows' },
      ),
    ).toBe(true)
    expect(matchesPlatform(desktopWindows, ['linux', 'windows'])).toBe(true)
  })
  test('selects the matching implementation, with OS before surface and cases ordered', () => {
    expect(
      matchPlatform(desktopWindows, {
        desktop: () => 'desktop',
        windows: () => 'windows',
      }),
    ).toBe('windows')
    expect(
      matchPlatform(desktopWindows, {
        cases: [
          { when: 'desktop', run: () => 'first' },
          { when: 'windows', run: () => 'second' },
        ],
      }),
    ).toBe('first')
  })
  test('runs only the selected branch and preserves default behavior', () => {
    const selected = vi.fn(() => 2)
    const fallback = vi.fn(() => 3)
    expect(
      matchPlatform(
        { surface: 'unknown', clientOs: 'unknown' },
        { browser: selected, default: fallback },
      ),
    ).toBe(3)
    expect(selected).not.toHaveBeenCalled()
    expect(fallback).toHaveBeenCalledOnce()
  })
})

describe('runtime helpers', () => {
  test('recognizes desktop operating systems without build-host hints', () => {
    expect(getClientOs('Mozilla Windows NT 10.0')).toBe('windows')
    expect(getClientOs('Macintosh; Intel Mac OS X')).toBe('macos')
    expect(getClientOs('X11; Linux x86_64')).toBe('linux')
    expect(getClientOs('Android 14')).toBe('unknown')
    expect(getClientOs('unknown')).toBe('unknown')
  })
  beforeEach(() => {
    mocks.isTauri.mockReset()
    mocks.getCurrentWebviewWindow.mockReset()
  })
  test('does not obtain native window resources without the bridge', () => {
    expect(createWindowAdapter('none')).toBeNull()
    expect(mocks.getCurrentWebviewWindow).not.toHaveBeenCalled()
  })

  test('creates the native window adapter only when given the bridge', async () => {
    const nativeWindow = {
      label: 'main',
      close: vi.fn().mockResolvedValue(undefined),
      show: vi.fn().mockResolvedValue(undefined),
      minimize: vi.fn().mockResolvedValue(undefined),
      maximize: vi.fn().mockResolvedValue(undefined),
      setAlwaysOnTop: vi.fn().mockResolvedValue(undefined),
      isMaximized: vi.fn().mockResolvedValue(true),
      isFullscreen: vi.fn().mockResolvedValue(false),
      toggleMaximize: vi.fn().mockResolvedValue(undefined),
      isMinimized: vi.fn().mockResolvedValue(false),
      theme: vi.fn().mockResolvedValue('dark'),
      setTheme: vi.fn().mockResolvedValue(undefined),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    }
    mocks.getCurrentWebviewWindow.mockReturnValue(nativeWindow)

    const adapter = createWindowAdapter('tauri')

    expect(mocks.getCurrentWebviewWindow).toHaveBeenCalledOnce()
    expect(adapter?.label()).toBe('main')
    await expect(adapter?.isMaximized()).resolves.toBe(true)
    await adapter?.setAlwaysOnTop(true)
    expect(nativeWindow.setAlwaysOnTop).toHaveBeenCalledWith(true)
  })
})
