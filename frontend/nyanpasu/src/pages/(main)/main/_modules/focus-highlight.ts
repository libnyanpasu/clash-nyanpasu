import { useEffect, useRef, useState } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'

/** How long the item a page was opened or returned to stays highlighted. */
export const FOCUS_HIGHLIGHT_MS = 2_000

// The slow fade belongs to the highlight alone, so hover and press feedback
// on the rows keep their quick transitions. Keyframes in tailwind.css.
export const focusHighlightStyle = {
  animation: `focus-highlight ${FOCUS_HIGHLIGHT_MS}ms ease-in`,
}

/**
 * Scrolls the list to the item whose key is `focus` once it appears and
 * highlights it for a while; returns the highlighted key. Items arrive after
 * the page mounts, so the focus waits for its item, and is applied once.
 */
export function useFocusHighlight<T>(
  focus: string | undefined,
  items: readonly T[],
  keyOf: (item: T) => string | undefined,
  virtualizer: Pick<
    Virtualizer<Element, Element>,
    'scrollElement' | 'scrollToIndex'
  >,
) {
  const [highlighted, setHighlighted] = useState<string>()

  const focusDone = useRef(false)

  // A list mounted together with its scroll area: the virtualizer adopts the
  // viewport only after the first commit, and cannot scroll before.
  const { scrollElement } = virtualizer

  useEffect(() => {
    if (focusDone.current || !focus || !scrollElement) {
      return
    }

    const index = items.findIndex((item) => keyOf(item) === focus)

    if (index === -1) {
      return
    }

    focusDone.current = true
    virtualizer.scrollToIndex(index, { align: 'center' })
    setHighlighted(focus)
  }, [focus, items, keyOf, virtualizer, scrollElement])

  useEffect(() => {
    if (highlighted === undefined) {
      return
    }

    const timer = setTimeout(
      () => setHighlighted(undefined),
      FOCUS_HIGHLIGHT_MS,
    )

    return () => clearTimeout(timer)
  }, [highlighted])

  return highlighted
}
