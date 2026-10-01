import { useEffect, useRef } from 'react'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
} from '@/components/ui/modal'
import { ScrollArea, useScrollAreaViewport } from '@/components/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  useTrafficUsagePages,
  type Dimension,
  type TrafficQuery,
} from '@nyanpasu/interface'
import Notice from './notice'
import RankingRow from './ranking-row'
import type { UsageLabel } from './usage-label'

// The next page loads when the end of the list scrolls into view.
function LoadMore({ onVisible }: { onVisible: () => void }) {
  const { viewportRef } = useScrollAreaViewport()

  const end = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const target = end.current

    if (!target) {
      return
    }

    const observer = new IntersectionObserver(
      ([entry]) => entry.isIntersecting && onVisible(),
      { root: viewportRef.current },
    )

    observer.observe(target)

    return () => observer.disconnect()
  }, [onVisible, viewportRef])

  return <div ref={end} aria-hidden className="h-px" />
}

function UsageList({
  query,
  dimension,
  selected,
  labelOf,
  onSelect,
}: {
  query: TrafficQuery
  dimension: Dimension
  selected?: string
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSelect: (key: string) => void
}) {
  const { data, isError, hasNextPage, isFetchingNextPage, fetchNextPage } =
    useTrafficUsagePages(query, dimension)

  const groups = data?.pages.flatMap((page) => page.groups) ?? []

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-1 px-2">
        {groups.map((group, index) => (
          <RankingRow
            key={group.key}
            label={labelOf(dimension, group.key)}
            usage={group.usage}
            first={index === 0}
            active={group.key === selected}
            onSelect={() => onSelect(group.key)}
          />
        ))}

        {hasNextPage && !isFetchingNextPage && (
          <LoadMore onVisible={fetchNextPage} />
        )}

        {isError ? (
          <Notice error>{m.traffic_unavailable()}</Notice>
        ) : !data || isFetchingNextPage ? (
          <p className="text-on-surface-variant px-4 py-3 text-sm">
            {m.traffic_loading_more()}
          </p>
        ) : (
          groups.length === 0 && <Notice>{m.traffic_empty()}</Notice>
        )}
      </div>
    </ScrollArea>
  )
}

export default function RankingModal({
  open,
  onOpenChange,
  title,
  query,
  dimension,
  selected,
  labelOf,
  onSelect,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  query: TrafficQuery
  dimension: Dimension
  selected?: string
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSelect: (key: string) => void
}) {
  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalContent>
        <div className="bg-surface text-on-surface flex max-h-[85dvh] w-[min(32rem,calc(100vw-2rem))] flex-col gap-2 rounded-3xl p-4">
          <ModalTitle className="px-4 pt-2 text-xl">{title}</ModalTitle>

          <UsageList
            query={query}
            dimension={dimension}
            selected={selected}
            labelOf={labelOf}
            onSelect={onSelect}
          />

          <div className="flex justify-end">
            <ModalClose>{m.common_close()}</ModalClose>
          </div>
        </div>
      </ModalContent>
    </Modal>
  )
}
