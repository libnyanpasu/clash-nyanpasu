import { AnimatePresence } from 'motion/react'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card } from '@nyanpasu/ui/card'
import { m } from '@/paraglide/messages'
import { usageAmount } from '@/utils/traffic-usage'
import type { Metric } from '@nyanpasu/rpc/types'
import { setFilter, toggleFilter } from '../../_modules/traffic-filters'
import type { RankingCardProps } from './ranking-card'
import RankingModal from './ranking-modal'
import RankingRow from './ranking-row'

const metricLabel = (metric: Metric) =>
  metric === 'bytes' ? m.traffic_stat_total() : m.traffic_stat_connections()

export function RankingView({
  icon: Icon,
  title,
  ranking,
  query,
  metric,
  filters,
  labelOf,
  onFiltersChange,
}: RankingCardProps) {
  const [viewAll, setViewAll] = useState(false)

  const { dimension } = ranking
  const selected = filters.find((filter) => filter.d === dimension)?.v
  const others = ranking.distinct - ranking.groups.length
  const secondaryMetric = metric === 'bytes' ? 'connections' : 'bytes'

  return (
    <Card
      className="@container flex w-full min-w-0 flex-col gap-1.5 p-3"
      data-slot="traffic-ranking-view"
    >
      <div className="flex min-w-0 items-center gap-1.5 px-1">
        <Icon className="text-on-surface-variant size-4 shrink-0" />

        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          <h2 className="truncate text-sm font-medium">{title}</h2>

          <span className="bg-on-surface/8 text-on-surface-variant rounded-full px-1.5 text-[11px] leading-4 tabular-nums">
            {ranking.distinct.toLocaleString()}
          </span>
        </div>

        <Button
          className="h-7 shrink-0 px-2 text-xs whitespace-nowrap"
          onClick={() => setViewAll(true)}
        >
          {m.traffic_rank_view_all()}
        </Button>
      </div>

      <div
        aria-hidden="true"
        className="text-on-surface-variant hidden grid-cols-[minmax(0,2fr)_repeat(4,minmax(0,1fr))] items-center gap-2 px-2 text-xs @lg:grid"
      >
        <span />
        <span
          className="truncate text-right"
          title={m.connections_field_upload()}
        >
          {m.connections_field_upload()}
        </span>
        <span
          className="truncate text-right"
          title={m.connections_field_download()}
        >
          {m.connections_field_download()}
        </span>
        <span
          className="truncate text-right"
          title={metricLabel(secondaryMetric)}
        >
          {metricLabel(secondaryMetric)}
        </span>
        <span className="truncate text-right" title={metricLabel(metric)}>
          {metricLabel(metric)}
        </span>
      </div>

      <div className="flex min-w-0 flex-col gap-0.5">
        <AnimatePresence initial={false}>
          {ranking.groups.map((group, index) => (
            <RankingRow
              key={group.key}
              label={labelOf(dimension, group.key)}
              usage={group.usage}
              metric={metric}
              rank={index}
              active={group.key === selected}
              layout="table"
              onSelect={() =>
                onFiltersChange(toggleFilter(filters, dimension, group.key))
              }
            />
          ))}
        </AnimatePresence>

        {others > 0 && (
          <p className="text-on-surface-variant truncate px-3 py-1.5 text-[11px] tabular-nums">
            {m.traffic_rank_other({ count: others.toLocaleString() })} ·{' '}
            {usageAmount(ranking.other, metric)}
          </p>
        )}
      </div>

      <RankingModal
        open={viewAll}
        onOpenChange={setViewAll}
        title={title}
        dimension={dimension}
        metric={metric}
        // Every value of the dimension is a choice, so its own filter stays off.
        query={{
          ...query,
          filters: query.filters.filter(
            (filter) => filter.dimension !== dimension,
          ),
        }}
        selected={selected}
        labelOf={labelOf}
        onSelect={(key) => {
          onFiltersChange(setFilter(filters, dimension, key))
          setViewAll(false)
        }}
      />
    </Card>
  )
}

export default RankingView
