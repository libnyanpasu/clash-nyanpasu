import { useEffect, useRef } from 'react'
import type { ClientOs } from '@nyanpasu/platform'
import { usePlatform } from '@nyanpasu/platform/react'

/** A key with its modifiers; `mod` is Command on macOS and Control elsewhere. */
export type Shortcut = {
  /** The physical key, as `KeyboardEvent.code` names it. */
  code: string
  mod?: boolean
  shift?: boolean
  alt?: boolean
}

export const EDITOR_SHORTCUTS = {
  apply: { code: 'KeyS', mod: true },
  save: { code: 'KeyS', mod: true, shift: true },
  close: { code: 'KeyW', mod: true },
  wordWrap: { code: 'KeyZ', alt: true },
  // Monaco binds these itself; they are listed to label the toolbar.
  undo: { code: 'KeyZ', mod: true },
  redo: { code: 'KeyZ', mod: true, shift: true },
  find: { code: 'KeyF', mod: true },
  commandPalette: { code: 'F1' },
} as const satisfies Record<string, Shortcut>

/** Monaco formats a document with a different key on Linux. */
export const formatShortcut = (clientOs: ClientOs): Shortcut =>
  clientOs === 'linux'
    ? { code: 'KeyI', mod: true, shift: true }
    : { code: 'KeyF', shift: true, alt: true }

type ShortcutEvent = Pick<
  KeyboardEvent,
  'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'
>

export function matchesShortcut(
  event: ShortcutEvent,
  { code, mod = false, shift = false, alt = false }: Shortcut,
  isMacos: boolean,
) {
  const modKey = isMacos ? event.metaKey : event.ctrlKey
  const otherKey = isMacos ? event.ctrlKey : event.metaKey

  return (
    event.code === code &&
    modKey === mod &&
    !otherKey &&
    event.shiftKey === shift &&
    event.altKey === alt
  )
}

/** How a shortcut is written on the platform, such as `⇧⌘S` or `Ctrl+Shift+S`. */
export function shortcutLabel(
  { code, mod, shift, alt }: Shortcut,
  isMacos: boolean,
) {
  const key = code.replace(/^Key/, '')

  if (isMacos) {
    return `${alt ? '⌥' : ''}${shift ? '⇧' : ''}${mod ? '⌘' : ''}${key}`
  }

  return [mod && 'Ctrl', shift && 'Shift', alt && 'Alt', key]
    .filter(Boolean)
    .join('+')
}

export type ShortcutBinding = readonly [shortcut: Shortcut, run: () => void]

/**
 * Runs the bound action of a shortcut pressed anywhere in the window, also
 * while the editor has the focus.
 */
export function useShortcuts(bindings: readonly ShortcutBinding[]) {
  const isMacos = usePlatform().clientOs === 'macos'

  const bindingsRef = useRef(bindings)
  bindingsRef.current = bindings

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const binding = bindingsRef.current.find(([shortcut]) =>
        matchesShortcut(event, shortcut, isMacos),
      )

      if (binding) {
        event.preventDefault()
        event.stopPropagation()
        binding[1]()
      }
    }

    // Captured, so the editor does not handle the key first.
    window.addEventListener('keydown', handleKeyDown, true)

    return () => window.removeEventListener('keydown', handleKeyDown, true)
  }, [isMacos])
}
