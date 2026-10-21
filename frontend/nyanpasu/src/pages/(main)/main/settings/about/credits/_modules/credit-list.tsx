import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { useScrollAreaViewportElement } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { useVirtualizer } from '@tanstack/react-virtual'
import CreditCountBadge from './credit-count-badge'
import { creditSourceLabel } from './credit-labels'
import type { CreditPackage, CreditSource } from './use-open-source-credits'

const CREDIT_ROW_HEIGHT = 56

const CREDIT_SECTION_HEIGHT = 48

type CreditListItem =
  | { type: 'section'; source: CreditSource; count: number }
  | { type: 'package'; pkg: CreditPackage }

function CreditSection({
  source,
  count,
  style,
}: {
  source: CreditSource
  count: number
  style: CSSProperties
}) {
  return (
    <div
      className="absolute inset-x-0 top-0 flex items-center gap-2 px-4 pt-3"
      style={{ ...style, height: CREDIT_SECTION_HEIGHT }}
      data-slot="credits-section"
      data-source={source}
    >
      <h3 className="text-primary text-sm font-medium">
        {creditSourceLabel[source]()}
      </h3>

      <CreditCountBadge count={count} />
    </div>
  )
}

function CreditRow({
  pkg,
  onSelect,
  style,
}: {
  pkg: CreditPackage
  onSelect: (pkg: CreditPackage) => void
  style: CSSProperties
}) {
  return (
    <button
      type="button"
      className={cn(
        'absolute inset-x-0 top-0 flex items-center gap-3 rounded-2xl px-4 text-left',
        'hover:bg-on-surface/8 focus-visible:outline-primary outline-none focus-visible:outline-2',
        'cursor-pointer transition-colors',
      )}
      style={{ ...style, height: CREDIT_ROW_HEIGHT }}
      onClick={() => onSelect(pkg)}
      data-slot="credits-row"
      data-source={pkg.source}
    >
      <div className="flex min-w-0 flex-1 items-baseline gap-2">
        <span className="truncate text-sm font-medium">{pkg.name}</span>

        {pkg.version && (
          <span className="text-on-surface-variant shrink-0 text-xs tabular-nums">
            {pkg.version}
          </span>
        )}
      </div>

      <span
        className={cn(
          'bg-surface-variant/60 dark:bg-surface-variant/30 max-w-[45%] truncate rounded-full px-2.5 py-0.5 text-xs',
          !pkg.license && 'text-on-surface-variant',
        )}
        data-slot="credits-row-license"
      >
        {pkg.license ?? m.settings_about_credits_no_license()}
      </span>

      <ChevronRightRounded className="text-on-surface-variant size-5 shrink-0" />
    </button>
  )
}

/**
 * Lists the packages virtually inside the settings scroll area, under a
 * heading for each source; the packages arrive grouped by source. The content
 * above the list only changes height when the toolbar wraps, which follows the
 * viewport's width, so its offset is measured again when the viewport resizes.
 */
export default function CreditList({
  packages,
  onSelect,
}: {
  packages: CreditPackage[]
  onSelect: (pkg: CreditPackage) => void
}) {
  const viewportElement = useScrollAreaViewportElement()

  const listRef = useRef<HTMLDivElement>(null)

  const [scrollMargin, setScrollMargin] = useState(0)

  const items = useMemo(() => {
    const result: CreditListItem[] = []
    let section: Extract<CreditListItem, { type: 'section' }> | undefined

    for (const pkg of packages) {
      if (section?.source !== pkg.source) {
        section = { type: 'section', source: pkg.source, count: 0 }
        result.push(section)
      }

      section.count++
      result.push({ type: 'package', pkg })
    }

    return result
  }, [packages])

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => viewportElement,
    estimateSize: (index) =>
      items[index].type === 'section'
        ? CREDIT_SECTION_HEIGHT
        : CREDIT_ROW_HEIGHT,
    overscan: 8,
    scrollMargin,
  })

  useLayoutEffect(() => {
    const list = listRef.current

    if (!list || !viewportElement) {
      return
    }

    const measure = () =>
      setScrollMargin(
        list.getBoundingClientRect().top -
          viewportElement.getBoundingClientRect().top +
          viewportElement.scrollTop,
      )

    measure()

    const observer = new ResizeObserver(measure)

    observer.observe(viewportElement)

    return () => observer.disconnect()
  }, [viewportElement])

  return (
    <div
      ref={listRef}
      className="relative"
      style={{ height: virtualizer.getTotalSize() }}
      data-slot="credits-list"
    >
      {virtualizer.getVirtualItems().map((virtualItem) => {
        const item = items[virtualItem.index]
        const style = {
          transform: `translateY(${virtualItem.start - scrollMargin}px)`,
        }

        return item.type === 'section' ? (
          <CreditSection
            key={virtualItem.key}
            source={item.source}
            count={item.count}
            style={style}
          />
        ) : (
          <CreditRow
            key={virtualItem.key}
            pkg={item.pkg}
            onSelect={onSelect}
            style={style}
          />
        )
      })}
    </div>
  )
}
