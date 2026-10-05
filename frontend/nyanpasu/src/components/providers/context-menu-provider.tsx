import ContentCopy from '~icons/material-symbols/content-copy-rounded'
import ContentCut from '~icons/material-symbols/content-cut-rounded'
import ContentPaste from '~icons/material-symbols/content-paste-rounded'
import {
  Children,
  cloneElement,
  createContext,
  PropsWithChildren,
  ReactElement,
  ReactNode,
  Ref,
  RefCallback,
  RefObject,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from '@nyanpasu/ui/context-menu'
import { m } from '@/paraglide/messages'
import { readClipboardText, writeClipboardText } from '@/utils/clipboard'

type ContextMenuRegistryValue = {
  registerElement: (el: Element, getChildren: () => ReactNode) => void
  unregisterElement: (el: Element) => void
}

const ContextMenuRegistryContext =
  createContext<ContextMenuRegistryValue | null>(null)

const useContextMenuRegistry = () => {
  const context = useContext(ContextMenuRegistryContext)

  if (!context) {
    throw new Error(
      'useContextMenuRegistry must be used within a ContextMenuRegistryContext',
    )
  }

  return context
}

export function useRegisterContextMenu<T extends Element>(
  menuChildren: ReactNode,
): RefCallback<T> {
  const { registerElement, unregisterElement } = useContextMenuRegistry()

  const elementRef = useRef<T | null>(null)

  const childrenRef = useRef(menuChildren)

  childrenRef.current = menuChildren

  const getChildren = useCallback(() => childrenRef.current, [])

  return useCallback(
    (el: T | null) => {
      if (elementRef.current) {
        unregisterElement(elementRef.current)
        elementRef.current = null
      }

      if (el) {
        elementRef.current = el
        registerElement(el, getChildren)
      }
    },
    [registerElement, unregisterElement, getChildren],
  )
}

type RegisterContextMenuInternalCtxValue = {
  childrenRef: RefObject<ReactNode>
  setTriggerEl: (el: Element | null) => void
}

const RegisterContextMenuInternalCtx =
  createContext<RegisterContextMenuInternalCtxValue | null>(null)

const useRegisterContextMenuInternal = () => {
  const ctx = useContext(RegisterContextMenuInternalCtx)

  if (!ctx) {
    throw new Error(
      'RegisterContextMenuTrigger/Content must be used within RegisterContextMenu',
    )
  }

  return ctx
}

export function RegisterContextMenu({ children }: PropsWithChildren) {
  const { registerElement, unregisterElement } = useContextMenuRegistry()

  const triggerElRef = useRef<Element | null>(null)
  const childrenRef = useRef<ReactNode>(null)

  const getChildren = useCallback(() => childrenRef.current, [])

  const setTriggerEl = useCallback(
    (el: Element | null) => {
      if (triggerElRef.current) {
        unregisterElement(triggerElRef.current)
      }
      triggerElRef.current = el
      if (el) {
        registerElement(el, getChildren)
      }
    },
    [registerElement, unregisterElement, getChildren],
  )

  const value = useMemo(() => ({ childrenRef, setTriggerEl }), [setTriggerEl])

  return (
    <RegisterContextMenuInternalCtx.Provider value={value}>
      {children}
    </RegisterContextMenuInternalCtx.Provider>
  )
}

/**
 * Attaches context-menu registration to its child element.
 *
 * - `asChild` (default `false`): wraps children in a `<span>`.
 * - `asChild={true}`: merges the registration ref directly into the single
 *   child element, preserving any existing ref on it.
 */
export function RegisterContextMenuTrigger({
  children,
  asChild = false,
}: {
  children: ReactElement
  asChild?: boolean
}) {
  const { setTriggerEl } = useRegisterContextMenuInternal()

  // For asChild: keep the child's original ref in a stable container so
  // mergedRef doesn't have it as a dep and stays stable across renders.
  const child = Children.only(children) as ReactElement<{
    ref?: Ref<Element>
  }>
  const originalRefLatest = useRef<Ref<Element> | undefined>(child.props.ref)
  originalRefLatest.current = child.props.ref

  const mergedRef = useCallback(
    (el: Element | null) => {
      setTriggerEl(el)
      const orig = originalRefLatest.current

      if (typeof orig === 'function') {
        orig(el)
      } else if (orig != null) {
        ;(orig as RefObject<Element | null>).current = el
      }
    },
    [setTriggerEl],
  )

  if (asChild) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return cloneElement(child, { ref: mergedRef } as any)
  }

  return <span ref={setTriggerEl}>{children}</span>
}

export function RegisterContextMenuContent({ children }: PropsWithChildren) {
  const { childrenRef } = useRegisterContextMenuInternal()

  // Update the ref synchronously during render so getChildren() always returns
  // the latest JSX when the menu opens (safe — mutating a ref, not state).
  childrenRef.current = children

  useEffect(() => {
    // Also set in the effect body so that React StrictMode's cleanup+re-invoke
    // cycle restores the value after the cleanup sets it to null. Later
    // renders update it above, so it runs on mount and unmount only.
    childrenRef.current = children

    return () => {
      childrenRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [childrenRef])

  return null
}

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

  const [customChildren, setCustomChildren] = useState<ReactNode>(null)

  const targetRef = useRef<Element | null>(null)

  const [open, setOpen] = useState(false)

  const [selectionRects, setSelectionRects] = useState<
    Array<{ left: number; top: number; width: number; height: number }>
  >([])

  const registryRef = useRef(new Map<Element, () => ReactNode>())

  const lastRightClickTargetRef = useRef<Element | null>(null)
  const rightClickSelectionRef = useRef('')
  const rightClickRangeRef = useRef<Range | null>(null)

  // Capture the right-clicked element before the context menu opens.
  // Use pointerdown (button === 2) instead of contextmenu so the target is
  // always recorded before any contextmenu listener (including Radix's) fires.
  useEffect(() => {
    const handler = (e: PointerEvent) => {
      if (e.button === 2) {
        lastRightClickTargetRef.current = e.target as Element
        const selection = window.getSelection()
        rightClickSelectionRef.current = selection?.toString() ?? ''
        rightClickRangeRef.current =
          selection && selection.rangeCount > 0
            ? selection.getRangeAt(0).cloneRange()
            : null
      }
    }
    document.addEventListener('pointerdown', handler, true)
    return () => document.removeEventListener('pointerdown', handler, true)
  }, [])

  useEffect(() => {
    if (!open || !rightClickRangeRef.current) {
      return
    }

    const updateSelectionRects = () => {
      const range = rightClickRangeRef.current
      if (!range) {
        return
      }

      const textNodes: Text[] = []
      if (range.commonAncestorContainer.nodeType === Node.TEXT_NODE) {
        textNodes.push(range.commonAncestorContainer as Text)
      } else {
        const walker = document.createTreeWalker(
          range.commonAncestorContainer,
          NodeFilter.SHOW_TEXT,
        )
        let node = walker.nextNode()
        while (node) {
          textNodes.push(node as Text)
          node = walker.nextNode()
        }
      }

      const rects: Array<{
        left: number
        top: number
        width: number
        height: number
      }> = []

      for (const node of textNodes) {
        if (!range.intersectsNode(node)) {
          continue
        }

        const nodeRange = document.createRange()
        const start = range.startContainer === node ? range.startOffset : 0
        const end =
          range.endContainer === node
            ? range.endOffset
            : (node.textContent?.length ?? 0)
        if (start >= end) {
          continue
        }

        nodeRange.setStart(node, start)
        nodeRange.setEnd(node, end)
        rects.push(
          ...Array.from(
            nodeRange.getClientRects(),
            ({ left, top, width, height }) => ({ left, top, width, height }),
          ),
        )
      }

      setSelectionRects(rects)
    }

    updateSelectionRects()

    window.addEventListener('resize', updateSelectionRects)
    window.addEventListener('scroll', updateSelectionRects, true)

    return () => {
      window.removeEventListener('resize', updateSelectionRects)
      window.removeEventListener('scroll', updateSelectionRects, true)
    }
  }, [open])

  const registerElement = useCallback(
    (el: Element, getChildren: () => ReactNode) => {
      registryRef.current.set(el, getChildren)
    },
    [],
  )

  const unregisterElement = useCallback((el: Element) => {
    registryRef.current.delete(el)
  }, [])

  const handleOpenChange = useCallback((nextOpen: boolean) => {
    setOpen(nextOpen)

    if (!nextOpen) {
      rightClickSelectionRef.current = ''
      rightClickRangeRef.current = null
      setSelectionRects([])
      return
    }

    const selection = window.getSelection()
    const selectedText = selection?.toString() || rightClickSelectionRef.current
    rightClickSelectionRef.current = selectedText
    setHasSelection(selectedText.length > 0)

    const active = document.activeElement
    setEditable(isEditable(active))
    targetRef.current = active

    // Traverse up the DOM from the right-clicked element to find registered children.
    let el: Element | null = lastRightClickTargetRef.current
    let found: ReactNode = null
    while (el) {
      const getter = registryRef.current.get(el)
      if (getter) {
        found = getter()
        break
      }
      el = el.parentElement
    }
    setCustomChildren(found)
  }, [])

  const handleCopy = useCallback(async () => {
    const text = rightClickSelectionRef.current

    if (!text) {
      return
    }

    await writeClipboardText(text)
  }, [])

  const handleCut = useCallback(async () => {
    const selection = window.getSelection()
    const text = rightClickSelectionRef.current
    const range = rightClickRangeRef.current

    if (!text || !editable) {
      return
    }

    await writeClipboardText(text)

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

    if (el instanceof HTMLElement && el.isContentEditable && range) {
      range.deleteContents()
      range.collapse(true)
      selection?.removeAllRanges()
      selection?.addRange(range)
    }
  }, [editable])

  const handlePaste = useCallback(async () => {
    try {
      const text = await readClipboardText()
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
    <ContextMenuRegistryContext.Provider
      value={{ registerElement, unregisterElement }}
    >
      <ContextMenu open={open} onOpenChange={handleOpenChange}>
        <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>

        <ContextMenuContent>
          {customChildren != null && (
            <>
              {customChildren}

              <ContextMenuSeparator />
            </>
          )}

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
      {selectionRects.length > 0 &&
        createPortal(
          <div
            aria-hidden="true"
            className="pointer-events-none fixed inset-0"
            style={{ zIndex: 49 }}
          >
            {selectionRects.map((rect) => (
              <div
                key={`${rect.left}:${rect.top}:${rect.width}:${rect.height}`}
                className="absolute rounded-[1px]"
                style={{
                  left: rect.left,
                  top: rect.top,
                  width: rect.width,
                  height: rect.height,
                  backgroundColor: 'rgb(59 130 246 / 38%)',
                }}
              />
            ))}
          </div>,
          document.body,
        )}
    </ContextMenuRegistryContext.Provider>
  )
}
