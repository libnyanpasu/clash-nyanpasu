import { AnimatePresence } from 'motion/react'
import { useEffect, useMemo, useRef } from 'react'
import { useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import {
  useMockTrafficNow,
  useMockTrafficSetting,
} from '@/hooks/use-mock-traffic'
import { m } from '@/paraglide/messages'
import { useTrafficUsagePages } from '@nyanpasu/query'
import type { Dimension, UsageGroup } from '@nyanpasu/rpc/types'
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

function UsageRows({
  groups,
  dimension,
  selected,
  onSelect,
}: UsageListProps & { groups: UsageGroup[] }) {
  const { search, labelOf } = useTrafficPage()

  return (
    <AnimatePresence initial={false}>
      {groups.map((group, index) => (
        <RankingRow
          key={group.key}
          label={labelOf(dimension, group.key)}
          usage={group.usage}
          metric={search.metric}
          rank={index}
          active={group.key === selected}
          layout="table"
          onSelect={() => onSelect(group.key)}
        />
      ))}
    </AnimatePresence>
  )
}

function RecordedUsageList({ dimension, selected, onSelect }: UsageListProps) {
  const { query, search } = useTrafficPage()

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

  const groups = data?.pages.flatMap((page) => page.groups) ?? []

  return (
    <>
      <UsageRows
        groups={groups}
        dimension={dimension}
        selected={selected}
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
    </>
  )
}

// Dev builds only: every generated group at once, as of when the list opened.
function MockUsageList({ dimension, selected, onSelect }: UsageListProps) {
  const { query, search } = useTrafficPage()
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
