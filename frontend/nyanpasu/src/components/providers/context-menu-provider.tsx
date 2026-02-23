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
  useRef,
  useState,
} from 'react'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { m } from '@/paraglide/messages'
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'

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

  return (
    <RegisterContextMenuInternalCtx.Provider
      value={{ childrenRef, setTriggerEl }}
    >
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
    return () => {
      childrenRef.current = null
    }
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

  const registryRef = useRef(new Map<Element, () => ReactNode>())

  const lastRightClickTargetRef = useRef<Element | null>(null)

  // Capture the right-clicked element before the context menu opens.
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      lastRightClickTargetRef.current = e.target as Element
    }
    document.addEventListener('contextmenu', handler, true)
    return () => document.removeEventListener('contextmenu', handler, true)
  }, [])

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

    if (nextOpen) {
      const selection = window.getSelection()
      setHasSelection(!!selection && selection.toString().length > 0)

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
    </ContextMenuRegistryContext.Provider>
  )
}
