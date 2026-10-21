import { useCallback } from 'react'
import { Card } from '@nyanpasu/ui/card'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import type { Metric } from '@nyanpasu/rpc/types'
import { createFileRoute, notFound } from '@tanstack/react-router'
import { RANKINGS, useTrafficPage } from '../_modules/traffic-context'
import { toggleFilter } from '../../_modules/traffic-filters'
import UsageList from './_modules/usage-list'

export const Route = createFileRoute('/(main)/main/topology/$dimension/')({
  params: {
    parse: ({ dimension }) => {
      const ranking = RANKINGS.find((item) => item.dimension === dimension)

      if (!ranking) throw notFound()

      return { dimension: ranking.dimension }
    },
    stringify: ({ dimension }) => ({ dimension }),
  },
  component: RankingsPage,
})

const metricLabel = (metric: Metric) =>
  metric === 'bytes' ? m.traffic_stat_total() : m.traffic_stat_connections()

function RankingsPage() {
  const { dimension } = Route.useParams()
  const { search, report, empty, onSearchChange } = useTrafficPage()
  const { metric, filters } = search

  const { icon: Icon, title } = RANKINGS.find(
    (item) => item.dimension === dimension,
  )!
  const distinct = report?.rankings.find(
    (item) => item.dimension === dimension,
  )?.distinct
  const selected = filters.find((filter) => filter.d === dimension)?.v
  // Stable across polls, so the listing does not re-render with each report.
  const onSelect = useCallback(
    (key: string) =>
      onSearchChange({ filters: toggleFilter(filters, dimension, key) }),
    [onSearchChange, filters, dimension],
  )
  const secondaryMetric = metric === 'bytes' ? 'connections' : 'bytes'

  return empty ? (
    <EmptyState className="min-h-40 flex-1" message={m.traffic_empty()} />
  ) : (
    <Card
      className="@container flex w-full min-w-0 flex-col gap-1.5 p-3"
      data-slot="traffic-ranking-list"
    >
      <div className="flex min-w-0 items-center gap-1.5 px-1">
        <Icon className="text-on-surface-variant size-4 shrink-0" />

        <h2 className="truncate text-sm font-medium">{title()}</h2>

        {distinct !== undefined && (
          <span className="bg-on-surface/8 text-on-surface-variant rounded-full px-1.5 text-[11px] leading-4 tabular-nums">
            {distinct.toLocaleString()}
          </span>
        )}
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
        <UsageList
          dimension={dimension}
          selected={selected}
          onSelect={onSelect}
        />
      </div>
    </Card>
  )
}
