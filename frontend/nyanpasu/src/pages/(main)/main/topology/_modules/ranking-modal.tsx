import { memo, useCallback, useEffect, useMemo, useRef } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import {
  ScrollArea,
  useScrollAreaViewport,
  useScrollAreaViewportElement,
} from '@nyanpasu/ui/scroll-area'
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
import { useVirtualizer } from '@tanstack/react-virtual'
import { setFilter } from '../../_modules/traffic-filters'
import { mockTrafficGroups } from './mock-traffic'
import Notice from './notice'
import RankingRow, { RankingRowTooltip } from './ranking-row'
import { useTrafficPage } from './traffic-context'

// A row's height before it is measured, and the space between rows.
const ROW_ESTIMATE = 54
const ROW_GAP = 4

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
  selected?: string
  onSelect: (key: string) => void
}

type LabelOf = (dimension: Dimension, key: string) => UsageLabel

/**
 * A row reads nothing from the page context and keeps its props across
 * renders, so neither a polled report nor another loaded page re-renders the
 * rows already shown. The listing is not polled, so its rows do not animate.
 */
const UsageRow = memo(function UsageRow({
  group,
  rank,
  active,
  dimension,
  metric,
  labelOf,
  onSelect,
}: {
  group: UsageGroup
  rank: number
  active: boolean
  dimension: Dimension
  metric: Metric
  labelOf: LabelOf
  onSelect: (key: string) => void
}) {
  return (
    <RankingRow
      label={labelOf(dimension, group.key)}
      usage={group.usage}
      metric={metric}
      rank={rank}
      active={active}
      animated={false}
      onSelect={() => onSelect(group.key)}
    />
  )
})

function UsageRows({
  groups,
  dimension,
  selected,
  onSelect,
  metric,
  labelOf,
}: Omit<UsageListProps, 'query'> & {
  groups: UsageGroup[]
  metric: Metric
  labelOf: LabelOf
}) {
  const viewport = useScrollAreaViewportElement()

  // Only the rows in view are mounted, so opening the listing and loading
  // more of it cost the same however many values it holds.
  const virtualizer = useVirtualizer({
    count: groups.length,
    getScrollElement: () => viewport,
    estimateSize: () => ROW_ESTIMATE,
    gap: ROW_GAP,
    overscan: 6,
    // Before the viewport is measured, enough rows to fill the window.
    initialRect: { width: 0, height: window.innerHeight },
  })

  const items = virtualizer.getVirtualItems()

  return (
    <div
      className="flex flex-col gap-1"
      style={{
        paddingTop: items[0]?.start ?? 0,
        paddingBottom: Math.max(
          0,
          virtualizer.getTotalSize() - (items.at(-1)?.end ?? 0),
        ),
      }}
      data-slot="traffic-usage-rows"
    >
      {items.map(({ index }) => {
        const group = groups[index]

        return (
          <div
            key={group.key}
            ref={virtualizer.measureElement}
            data-index={index}
          >
            <UsageRow
              group={group}
              rank={index}
              active={group.key === selected}
              dimension={dimension}
              metric={metric}
              labelOf={labelOf}
              onSelect={onSelect}
            />
          </div>
        )
      })}
    </div>
  )
}

function UsageList({ query, dimension, selected, onSelect }: UsageListProps) {
  const { search, labelOf } = useTrafficPage()
  const { metric } = search
  const { data, isError, hasNextPage, isFetchingNextPage, fetchNextPage } =
    useTrafficUsagePages(query, dimension, metric)

  const groups = useMemo(
    () => data?.pages.flatMap((page) => page.groups) ?? [],
    [data],
  )

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-1 px-2">
        <UsageRows
          groups={groups}
          dimension={dimension}
          selected={selected}
          onSelect={onSelect}
          metric={metric}
          labelOf={labelOf}
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
          groups.length === 0 && (
            <EmptyState
              tone="normal"
              className="min-h-40 flex-1"
              message={m.traffic_empty()}
            />
          )
        )}
      </div>
    </ScrollArea>
  )
}

// Dev builds only: every generated group at once, as of when the list opened.
function MockUsageList({
  query,
  dimension,
  selected,
  onSelect,
}: UsageListProps) {
  const { search, labelOf } = useTrafficPage()
  const { metric } = search
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
          selected={selected}
          onSelect={onSelect}
          metric={metric}
          labelOf={labelOf}
        />

        {groups.length === 0 && (
          <EmptyState className="min-h-40 flex-1" message={m.traffic_empty()} />
        )}
      </div>
    </ScrollArea>
  )
}

export default function RankingModal({
  open,
  onOpenChange,
  title,
  dimension,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  dimension: Dimension
}) {
  const [mocked] = useMockTrafficSetting()
  const { query, search, onSearchChange } = useTrafficPage()
  const { filters } = search
  const selected = filters.find((filter) => filter.d === dimension)?.v
  // The modal lists every value, so its own filter must not remove candidates.
  const rankingQuery = {
    ...query,
    filters: query.filters.filter((filter) => filter.dimension !== dimension),
  }
  // Stable across polls, so the listing does not re-render with each report.
  const handleSelect = useCallback(
    (key: string) => {
      onSearchChange({ filters: setFilter(filters, dimension, key) })
      onOpenChange(false)
    },
    [onSearchChange, filters, dimension, onOpenChange],
  )

  const List = mocked ? MockUsageList : UsageList

  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalContent>
        <div className="bg-surface text-on-surface flex max-h-[85dvh] w-[min(32rem,calc(100vw-2rem))] flex-col gap-2 rounded-3xl p-4">
          <ModalTitle className="px-4 pt-2 text-xl">{title}</ModalTitle>

          <RankingRowTooltip>
            <List
              query={rankingQuery}
              dimension={dimension}
              selected={selected}
              onSelect={handleSelect}
            />
          </RankingRowTooltip>

          <div className="flex justify-end">
            <ModalClose>{m.common_close()}</ModalClose>
          </div>
        </div>
      </ModalContent>
    </Modal>
  )
}
