import {
  memo,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import {
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
import type { Dimension, Metric, UsageGroup } from '@nyanpasu/rpc/types'
import { measureElement, useVirtualizer } from '@tanstack/react-virtual'
import { mockTrafficGroups } from '../../_modules/mock-traffic'
import Notice from '../../_modules/notice'
import RankingRow, { RankingRowTooltip } from '../../_modules/ranking-row'
import { useTrafficPage } from '../../_modules/traffic-context'

// A row's height before any is measured, and the space between rows.
const ROW_ESTIMATE = 36
const ROW_GAP = 2

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
      layout="table"
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
}: UsageListProps & {
  groups: UsageGroup[]
  metric: Metric
  labelOf: LabelOf
}) {
  const viewport = useScrollAreaViewportElement()

  const list = useRef<HTMLDivElement>(null)

  const [scrollMargin, setScrollMargin] = useState(0)

  // The rows wrap onto a second line in a narrow card, but at one width they
  // are all as tall, so the last measured row stands in for the rows not
  // mounted yet and the list's height is right from the first measurement.
  const rowHeight = useRef(ROW_ESTIMATE)

  // Only the rows in view are mounted, so opening the listing and loading
  // more of it cost the same however many values it holds.
  const virtualizer = useVirtualizer({
    count: groups.length,
    getScrollElement: () => viewport,
    estimateSize: () => rowHeight.current,
    measureElement: (element, entry, instance) =>
      (rowHeight.current = measureElement(element, entry, instance)),
    gap: ROW_GAP,
    overscan: 2,
    scrollMargin,
  })

  // The list starts below the page's heading, toolbar and dimension tabs,
  // which change height when the window resizes or a filter chip comes or
  // goes, so the offset is measured again whenever the page content resizes.
  useLayoutEffect(() => {
    const element = list.current
    const content = viewport?.firstElementChild

    if (!element || !viewport || !content) {
      return
    }

    const measure = () =>
      setScrollMargin(
        element.getBoundingClientRect().top -
          viewport.getBoundingClientRect().top +
          viewport.scrollTop,
      )

    measure()

    const observer = new ResizeObserver(measure)

    observer.observe(viewport)
    observer.observe(content)

    return () => observer.disconnect()
  }, [viewport])

  // The rows render in the pass after the page, which keeps their place at
  // the list's estimated height, so opening the page is not one long task.
  const rowsShown = useDeferredValue(true, false)
  const items = rowsShown ? virtualizer.getVirtualItems() : []

  return (
    <RankingRowTooltip>
      <div
        ref={list}
        className="flex min-w-0 flex-col gap-0.5"
        // Item positions count from the top of the page content, the total
        // size from the top of the list.
        style={{
          paddingTop: (items[0]?.start ?? scrollMargin) - scrollMargin,
          paddingBottom: Math.max(
            0,
            virtualizer.getTotalSize() -
              ((items.at(-1)?.end ?? scrollMargin) - scrollMargin),
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
    </RankingRowTooltip>
  )
}

function RecordedUsageList({ dimension, selected, onSelect }: UsageListProps) {
  const { query, search, labelOf } = useTrafficPage()

  // The backend ignores the filter on the listed dimension. Leaving it out of
  // the request keeps the loaded pages, and the place read, when a row is picked.
  const listQuery = useMemo(
    () => ({
      ...query,
      filters: query.filters.filter((filter) => filter.dimension !== dimension),
    }),
    [query, dimension],
  )

  const { data, isError, hasNextPage, isFetchingNextPage, fetchNextPage } =
    useTrafficUsagePages(listQuery, dimension, search.metric)

  const groups = useMemo(
    () => data?.pages.flatMap((page) => page.groups) ?? [],
    [data],
  )

  return (
    <>
      <UsageRows
        groups={groups}
        dimension={dimension}
        selected={selected}
        onSelect={onSelect}
        metric={search.metric}
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
    </>
  )
}

// Dev builds only: every generated group at once, as of when the list opened.
function MockUsageList({ dimension, selected, onSelect }: UsageListProps) {
  const { query, search, labelOf } = useTrafficPage()
  const { metric } = search
  const now = useMockTrafficNow(true)

  const groups = useMemo(
    () =>
      now === null ? [] : mockTrafficGroups(query, dimension, metric, now),
    [query, dimension, metric, now],
  )

  return (
    <>
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
    </>
  )
}

/** Every value of a dimension, heaviest first, a page at a time. */
export default function UsageList(props: UsageListProps) {
  const [mocked] = useMockTrafficSetting()

  const List = mocked ? MockUsageList : RecordedUsageList

  return <List {...props} />
}
