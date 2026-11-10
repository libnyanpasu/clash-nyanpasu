import { expect, test, vi } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { PlatformProvider } from '@nyanpasu/platform/react'
import {
  EDITOR_SHORTCUTS,
  formatShortcut,
  matchesShortcut,
  shortcutLabel,
  useShortcuts,
} from '../src/pages/(editor)/editor/_modules/shortcuts'

const key = (init: KeyboardEventInit) =>
  new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })

test('the mod key is Command on macOS and Control elsewhere', () => {
  const ctrlS = key({ code: 'KeyS', ctrlKey: true })
  const cmdS = key({ code: 'KeyS', metaKey: true })

  expect(matchesShortcut(ctrlS, EDITOR_SHORTCUTS.apply, false)).toBe(true)
  expect(matchesShortcut(ctrlS, EDITOR_SHORTCUTS.apply, true)).toBe(false)
  expect(matchesShortcut(cmdS, EDITOR_SHORTCUTS.apply, true)).toBe(true)
  expect(matchesShortcut(cmdS, EDITOR_SHORTCUTS.apply, false)).toBe(false)
})

test('an extra modifier is a different shortcut', () => {
  const ctrlShiftS = key({ code: 'KeyS', ctrlKey: true, shiftKey: true })

  expect(matchesShortcut(ctrlShiftS, EDITOR_SHORTCUTS.apply, false)).toBe(false)
  expect(matchesShortcut(ctrlShiftS, EDITOR_SHORTCUTS.save, false)).toBe(true)
})

test('a shortcut is written the way its platform writes it', () => {
  expect(shortcutLabel(EDITOR_SHORTCUTS.save, true)).toBe('⇧⌘S')
  expect(shortcutLabel(EDITOR_SHORTCUTS.save, false)).toBe('Ctrl+Shift+S')
  expect(shortcutLabel(EDITOR_SHORTCUTS.wordWrap, true)).toBe('⌥Z')
  expect(shortcutLabel(EDITOR_SHORTCUTS.commandPalette, false)).toBe('F1')
  expect(shortcutLabel(formatShortcut('windows'), false)).toBe('Shift+Alt+F')
  expect(shortcutLabel(formatShortcut('linux'), false)).toBe('Ctrl+Shift+I')
})

test('a bound shortcut runs its action and keeps the key from the page', async () => {
  const apply = vi.fn()
  const save = vi.fn()
  const hook = await renderHook(
    () =>
      useShortcuts([
        [EDITOR_SHORTCUTS.apply, apply],
        [EDITOR_SHORTCUTS.save, save],
      ]),
    {
      wrapper: ({ children }) => (
        <PlatformProvider platform={{ surface: 'desktop', clientOs: 'linux' }}>
          {children}
        </PlatformProvider>
      ),
    },
  )

  // Pressed inside an input, as when the editor has the focus.
  const input = document.createElement('textarea')
  document.body.append(input)

  const applyKey = key({ code: 'KeyS', ctrlKey: true })
  input.dispatchEvent(applyKey)
  expect(apply).toHaveBeenCalledTimes(1)
  expect(save).not.toHaveBeenCalled()
  expect(applyKey.defaultPrevented).toBe(true)

  const plainKey = key({ code: 'KeyS' })
  input.dispatchEvent(plainKey)
  expect(apply).toHaveBeenCalledTimes(1)
  expect(plainKey.defaultPrevented).toBe(false)

  await hook.unmount()
  input.dispatchEvent(key({ code: 'KeyS', ctrlKey: true }))
  expect(apply).toHaveBeenCalledTimes(1)
  input.remove()
})
