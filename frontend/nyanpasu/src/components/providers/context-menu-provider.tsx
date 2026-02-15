import ContentCopy from '~icons/material-symbols/content-copy-rounded'
import ContentCut from '~icons/material-symbols/content-cut-rounded'
import ContentPaste from '~icons/material-symbols/content-paste-rounded'
import { PropsWithChildren, useCallback, useRef, useState } from 'react'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { m } from '@/paraglide/messages'
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'

const isEditable = (el: Element | null): boolean => {
  if (!el || !(el instanceof HTMLElement)) {
    return false
  }

  const tag = el.tagName.toLowerCase()

  if (tag === 'input' || tag === 'textarea') {
    return true
  }

  if (el.isContentEditable) {
    return true
  }

  return false
}

export default function ContextMenuProvider({ children }: PropsWithChildren) {
  const [hasSelection, setHasSelection] = useState(false)

  const [editable, setEditable] = useState(false)

  const targetRef = useRef<Element | null>(null)

  const [open, setOpen] = useState(false)

  const handleOpenChange = useCallback((nextOpen: boolean) => {
    setOpen(nextOpen)

    if (nextOpen) {
      const selection = window.getSelection()
      setHasSelection(!!selection && selection.toString().length > 0)

      const active = document.activeElement
      setEditable(isEditable(active))
      targetRef.current = active
    }
  }, [])

  const handleCopy = useCallback(async () => {
    const selection = window.getSelection()
    const text = selection?.toString() ?? ''

    if (!text) {
      return
    }

    await writeText(text)
  }, [])

  const handleCut = useCallback(async () => {
    const selection = window.getSelection()
    const text = selection?.toString() ?? ''

    if (!text || !editable) {
      return
    }

    await writeText(text)

    const el = targetRef.current

    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
      const start = el.selectionStart ?? 0
      const end = el.selectionEnd ?? 0
      const currentValue = el.value

      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
        el instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype,
        'value',
      )?.set

      nativeInputValueSetter?.call(
        el,
        currentValue.slice(0, start) + currentValue.slice(end),
      )

      el.dispatchEvent(new Event('input', { bubbles: true }))
      el.setSelectionRange(start, start)
      return
    }

    if (el instanceof HTMLElement && el.isContentEditable && selection) {
      selection.deleteFromDocument()
    }
  }, [editable])

  const handlePaste = useCallback(async () => {
    try {
      const text = await readText()
      const el = targetRef.current

      if (el && isEditable(el)) {
        if (
          el instanceof HTMLInputElement ||
          el instanceof HTMLTextAreaElement
        ) {
          const start = el.selectionStart ?? 0
          const end = el.selectionEnd ?? 0
          const currentValue = el.value

          const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
            el instanceof HTMLTextAreaElement
              ? HTMLTextAreaElement.prototype
              : HTMLInputElement.prototype,
            'value',
          )?.set

          nativeInputValueSetter?.call(
            el,
            currentValue.slice(0, start) + text + currentValue.slice(end),
          )

          el.dispatchEvent(new Event('input', { bubbles: true }))

          const newPos = start + text.length
          el.setSelectionRange(newPos, newPos)
        } else {
          const editableEl = el as HTMLElement
          editableEl.focus()

          const selection = window.getSelection()
          if (!selection || selection.rangeCount === 0) {
            return
          }

          const range = selection.getRangeAt(0)
          range.deleteContents()
          range.insertNode(document.createTextNode(text))
          range.collapse(false)
          selection.removeAllRanges()
          selection.addRange(range)
        }
      }
    } catch {
      // Ignore clipboard read failures (e.g. permission denied).
    }
  }, [])

  return (
    <ContextMenu open={open} onOpenChange={handleOpenChange}>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>

      <ContextMenuContent>
        <ContextMenuItem
          disabled={!hasSelection || !editable}
          onSelect={handleCut}
        >
          <ContentCut className="size-4" />
          <span>{m.common_cut()}</span>
          <ContextMenuShortcut>Ctrl+X</ContextMenuShortcut>
        </ContextMenuItem>

        <ContextMenuItem disabled={!hasSelection} onSelect={handleCopy}>
          <ContentCopy className="size-4" />
          <span>{m.common_copy()}</span>
          <ContextMenuShortcut>Ctrl+C</ContextMenuShortcut>
        </ContextMenuItem>

        <ContextMenuItem disabled={!editable} onSelect={handlePaste}>
          <ContentPaste className="size-4" />
          <span>{m.common_paste()}</span>
          <ContextMenuShortcut>Ctrl+V</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}
