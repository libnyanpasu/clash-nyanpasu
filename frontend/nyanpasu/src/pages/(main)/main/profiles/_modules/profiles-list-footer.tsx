import { RefObject, useEffect, useState } from 'react'
import { useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'

export const NoMoreProfiles = () => {
  return (
    <div className="mb-4 flex h-16 items-center justify-center text-center text-sm text-gray-500">
      {m.profile_no_more_profiles()}
    </div>
  )
}

/**
 * Whether the last item of `listRef` reaches past the scroll viewport. The
 * footer is measured out on purpose: it would otherwise make its own room.
 */
export const useListOverflows = (
  listRef: RefObject<HTMLElement | null>,
  itemCount: number,
) => {
  const { viewportRef } = useScrollAreaViewport()

  const [overflows, setOverflows] = useState(false)

  useEffect(() => {
    const viewport = viewportRef.current
    const list = listRef.current

    if (!viewport || !list) {
      setOverflows(false)
      return
    }

    const update = () => {
      // CSS places the current profile first without changing sortable indices.
      const bottom =
        Array.from(list.children).reduce(
          (bottom, item) =>
            Math.max(bottom, item.getBoundingClientRect().bottom),
          list.getBoundingClientRect().top,
        ) -
        viewport.getBoundingClientRect().top +
        viewport.scrollTop

      setOverflows(bottom > viewport.clientHeight)
    }

    const observer = new ResizeObserver(update)
    observer.observe(viewport)
    observer.observe(list)

    return () => observer.disconnect()
  }, [viewportRef, listRef, itemCount])

  return overflows
}
