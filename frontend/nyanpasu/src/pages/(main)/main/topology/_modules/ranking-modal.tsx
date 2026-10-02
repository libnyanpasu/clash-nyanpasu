import { AnimatePresence } from 'motion/react'
import { useEffect, useMemo, useRef } from 'react'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { ScrollArea, useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import {
  useMockTrafficNow,
  useMockTrafficSetting,
} from '@/hooks/use-mock-traffic'
import { m } from '@/paraglide/messages'
import type { UsageLabel } from '@/utils/traffic-usage'
import { useTrafficUsagePages } from '@nyanpasu/query'
import {
  type Dimension,
  type Metric,
  type TrafficQuery,
  type UsageGroup,
} from '@nyanpasu/rpc/types'
import { mockTrafficGroups } from './mock-traffic'
import Notice from './notice'
import RankingRow from './ranking-row'

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

type UsageListProps = {
  query: TrafficQuery
  dimension: Dimension
  metric: Metric
  selected?: string
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSelect: (key: string) => void
}

function UsageRows({
  groups,
  dimension,
  metric,
  selected,
  labelOf,
  onSelect,
}: Omit<UsageListProps, 'query'> & { groups: UsageGroup[] }) {
  return (
    <AnimatePresence initial={false}>
      {groups.map((group, index) => (
        <RankingRow
          key={group.key}
          label={labelOf(dimension, group.key)}
          usage={group.usage}
          metric={metric}
          rank={index}
          active={group.key === selected}
          onSelect={() => onSelect(group.key)}
        />
      ))}
    </AnimatePresence>
  )
}

function UsageList({
  query,
  dimension,
  metric,
  selected,
  labelOf,
  onSelect,
}: UsageListProps) {
  const { data, isError, hasNextPage, isFetchingNextPage, fetchNextPage } =
    useTrafficUsagePages(query, dimension, metric)

  const groups = data?.pages.flatMap((page) => page.groups) ?? []

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-1 px-2">
        <UsageRows
          groups={groups}
          dimension={dimension}
          metric={metric}
          selected={selected}
          labelOf={labelOf}
          onSelect={onSelect}
        />

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

// Dev builds only: every generated group at once, as of when the list opened.
function MockUsageList({ query, dimension, metric, ...rows }: UsageListProps) {
  const now = useMockTrafficNow(true)

  const groups = useMemo(
    () =>
      now === null ? [] : mockTrafficGroups(query, dimension, metric, now),
    [query, dimension, metric, now],
  )

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-1 px-2">
        <UsageRows
          groups={groups}
          dimension={dimension}
          metric={metric}
          {...rows}
        />

        {groups.length === 0 && <Notice>{m.traffic_empty()}</Notice>}
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
  metric,
  selected,
  labelOf,
  onSelect,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  query: TrafficQuery
  dimension: Dimension
  metric: Metric
  selected?: string
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSelect: (key: string) => void
}) {
  const [mocked] = useMockTrafficSetting()

  const List = mocked ? MockUsageList : UsageList

  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalContent>
        <div className="bg-surface text-on-surface flex max-h-[85dvh] w-[min(32rem,calc(100vw-2rem))] flex-col gap-2 rounded-3xl p-4">
          <ModalTitle className="px-4 pt-2 text-xl">{title}</ModalTitle>

          <List
            query={query}
            dimension={dimension}
            metric={metric}
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
