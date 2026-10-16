import { memo, useEffect, useMemo, useRef } from 'react'
import { useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import {
  useMockTrafficNow,
  useMockTrafficSetting,
} from '@/hooks/use-mock-traffic'
import { m } from '@/paraglide/messages'
import type { UsageLabel } from '@/utils/traffic-usage'
import { useTrafficUsagePages } from '@nyanpasu/query'
import type { Dimension, Metric, UsageGroup } from '@nyanpasu/rpc/types'
import { mockTrafficGroups } from '../../_modules/mock-traffic'
import Notice from '../../_modules/notice'
import RankingRow from '../../_modules/ranking-row'
import { useTrafficPage } from '../../_modules/traffic-context'

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
  return groups.map((group, index) => (
    <UsageRow
      key={group.key}
      group={group}
      rank={index}
      active={group.key === selected}
      dimension={dimension}
      metric={metric}
      labelOf={labelOf}
      onSelect={onSelect}
    />
  ))
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
        groups.length === 0 && <Notice>{m.traffic_empty()}</Notice>
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

      {groups.length === 0 && <Notice>{m.traffic_empty()}</Notice>}
    </>
  )
}

/** Every value of a dimension, heaviest first, a page at a time. */
export default function UsageList(props: UsageListProps) {
  const [mocked] = useMockTrafficSetting()

  const List = mocked ? MockUsageList : RecordedUsageList

  return <List {...props} />
}
