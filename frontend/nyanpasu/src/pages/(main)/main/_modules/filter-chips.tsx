import { useLayoutEffect, useRef, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import FilterChip from './filter-chip'

export interface FilterChipItem {
  key: string
  /** "Dimension: value" */
  label: string
  title?: string
  onRemove: () => void
}

/**
 * How many chips stay inline: all of them when they fit with the clear-all
 * button, otherwise as many as fit before the chip that opens the rest,
 * which holds the clear-all button too.
 */
export function fitCount(
  chips: number[],
  clearAll: number,
  more: number,
  gap: number,
  width: number,
) {
  const all = chips.reduce((sum, chip) => sum + chip + gap, clearAll)

  if (all <= width) {
    return chips.length
  }

  let used = more
  let count = 0

  // At least the last chip moves, so the opener always hides one.
  for (const chip of chips.slice(0, -1)) {
    if (used + chip + gap > width) {
      break
    }

    used += chip + gap
    count += 1
  }

  return count
}

const chipButtonClass = 'h-8 shrink-0 px-3 whitespace-nowrap'

/** Removable filters on one line; those that do not fit open in a dialog. */
export default function FilterChips({
  items,
  onClearAll,
  className,
  'data-slot': dataSlot,
}: {
  items: FilterChipItem[]
  onClearAll: () => void
  className?: string
  'data-slot'?: string
}) {
  const containerRef = useRef<HTMLDivElement>(null)

  const [shown, setShown] = useState(items.length)

  const [open, setOpen] = useState(false)

  const labels = items.map((item) => item.label).join('\n')

  // What does not fit stays in place, invisible and at its natural width, so
  // the count follows both the space and the labels from the chips alone.
  useLayoutEffect(() => {
    const container = containerRef.current

    if (!container) {
      return
    }

    const children = [...container.children]

    const fit = () => {
      const widths = children.map(
        (child) => child.getBoundingClientRect().width,
      )
      const more = widths.pop() ?? 0
      const clearAll = widths.pop() ?? 0

      setShown(
        fitCount(
          widths,
          clearAll,
          more,
          parseFloat(getComputedStyle(container).columnGap) || 0,
          container.clientWidth,
        ),
      )
    }

    const observer = new ResizeObserver(fit)
    observer.observe(container)
    children.forEach((child) => observer.observe(child))

    return () => observer.disconnect()
  }, [labels])

  const hidden = items.length - shown

  // Neither takes room nor answers clicks; it is only measured.
  const unused = 'invisible absolute w-max'

  const showAll = m.traffic_filter_show_all({ count: items.length })

  const chip = (item: FilterChipItem, className?: string) => (
    <FilterChip
      key={item.key}
      className={className}
      label={item.label}
      title={item.title}
      onRemove={() => item.onRemove()}
    />
  )

  return (
    <div
      ref={containerRef}
      className={cn(
        'relative flex min-w-0 items-center gap-2 overflow-hidden',
        className,
      )}
      data-slot={dataSlot}
      role="group"
      aria-label={m.traffic_filters_label()}
    >
      {items.map((item, index) => chip(item, index < shown ? '' : unused))}

      {items.length > 0 && (
        <>
          <Button
            className={cn(chipButtonClass, hidden > 0 && unused)}
            onClick={onClearAll}
          >
            {m.traffic_filter_clear_all()}
          </Button>

          {/* Unused, it holds the widest count it could show. */}
          <Button
            className={cn(chipButtonClass, hidden === 0 && unused)}
            data-slot="filter-chips-more"
            aria-label={showAll}
            title={showAll}
            onClick={() => setOpen(true)}
          >
            {`+${hidden || items.length}`}
          </Button>
        </>
      )}

      <Modal open={open && items.length > 0} onOpenChange={setOpen}>
        <ModalContent>
          <Card divider className="flex w-96 max-w-[90vw] flex-col">
            <CardHeader>
              <ModalTitle>{m.traffic_filters_label()}</ModalTitle>
            </CardHeader>

            <CardContent
              className="flex flex-wrap gap-2"
              data-slot="filter-chips-dialog"
            >
              {items.map((item) => chip(item))}
            </CardContent>

            <CardFooter>
              <Button
                onClick={() => {
                  onClearAll()
                  setOpen(false)
                }}
              >
                {m.traffic_filter_clear_all()}
              </Button>

              <ModalClose variant="flat">{m.common_close()}</ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </div>
  )
}
