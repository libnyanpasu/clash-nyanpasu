import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DevicesRounded from '~icons/material-symbols/devices-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import LoginRounded from '~icons/material-symbols/login-rounded'
import { useMemo, useState, type ReactNode } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { useMockTrafficNow } from '@/hooks/use-mock-traffic'
import { m } from '@/paraglide/messages'
import { useSetting, useTrafficReport } from '@nyanpasu/query'
import { type Dimension, type ReportRequest } from '@nyanpasu/rpc/types'
import { toggleFilter } from '../../_modules/traffic-filters'
import { useUsageLabelOf } from '../../_modules/use-usage-label-of'
import { mockTrafficReport } from './mock-traffic'
import Notice from './notice'
import RankingCard from './ranking-card'
import {
  DEFAULT_TEMPLATE,
  pollInterval,
  toQuery,
  toTopologyRequest,
  type TrafficSearch,
} from './search'
import StatCards from './stat-cards'
import TopologyView from './topology-view'
import TrafficToolbar from './traffic-toolbar'

// One report serves the stat cards, every ranking card and the topology.
const RANKINGS = [
  { dimension: 'source', icon: DevicesRounded, title: m.traffic_rank_source },
  { dimension: 'inbound', icon: LoginRounded, title: m.traffic_rank_inbound },
  { dimension: 'target', icon: DnsRounded, title: m.traffic_rank_target },
  { dimension: 'exit', icon: CallSplitRounded, title: m.traffic_rank_exit },
  { dimension: 'process', icon: AppsRounded, title: m.traffic_rank_process },
] as const satisfies readonly {
  dimension: Dimension
  icon: unknown
  title: () => string
}[]

const RANKING_LIMIT = 5

export default function TrafficPage({
  search,
  onSearchChange,
  onViewConnections,
  toolbarStart,
}: {
  search: TrafficSearch
  onSearchChange: (update: Partial<TrafficSearch>) => void
  /** Opens the connections this page's scope, range and filters select. */
  onViewConnections?: () => void
  toolbarStart?: ReactNode
}) {
  const { range, scope, filters, view, layers, metric, limit } = search

  const [paused, setPaused] = useState(false)

  const { value: retention } = useSetting('traffic_retention')

  const labelOf = useUsageLabelOf()

  const query = useMemo(
    () => toQuery({ range, scope, filters }),
    [range, scope, filters],
  )

  // Only one of the flow and the map is on screen, so the report carries the
  // topology of that one. The metric orders the rankings and the topology alike.
  const request = useMemo<ReportRequest>(
    () => ({
      query,
      metric,
      rankings: RANKINGS.map(({ dimension }) => dimension),
      ranking_limit: RANKING_LIMIT,
      topology: toTopologyRequest({ view, layers, limit }),
    }),
    [query, metric, view, layers, limit],
  )

  // Dev builds only: generated usage replaces the recorded one.
  const mockNow = useMockTrafficNow(paused)

  const { data: recorded, isError: failed } = useTrafficReport(request, {
    refetchInterval: paused ? false : pollInterval(range),
    enabled: mockNow === null,
  })

  const mocked = useMemo(
    () => (mockNow === null ? undefined : mockTrafficReport(request, mockNow)),
    [request, mockNow],
  )

  const report = mocked ?? recorded
  const isError = !mocked && failed

  const empty =
    !!report &&
    report.total.connections === 0 &&
    report.total.bytes.upload + report.total.bytes.download === 0

  return (
    <div className="divide-outline-variant flex min-h-0 min-w-0 flex-1 flex-col divide-y overflow-hidden">
      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto max-w-7xl space-y-4 p-4">
          {isError ? (
            <Notice error>{m.traffic_unavailable()}</Notice>
          ) : (
            <StatCards report={report} />
          )}

          <TopologyView
            topology={report?.topology}
            view={view}
            onViewChange={(next) => onSearchChange({ view: next })}
            metric={metric}
            template={layers ?? DEFAULT_TEMPLATE}
            onTemplateChange={(next) => onSearchChange({ layers: next })}
            limit={limit}
            onLimitChange={(next) => onSearchChange({ limit: next })}
            filters={filters}
            labelOf={labelOf}
            onSelect={(dimension, key) =>
              onSearchChange({ filters: toggleFilter(filters, dimension, key) })
            }
          />

          {report &&
            !isError &&
            (empty ? (
              <Notice>{m.traffic_empty()}</Notice>
            ) : (
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
                {RANKINGS.map(({ dimension, icon, title }) => {
                  const ranking = report.rankings.find(
                    (item) => item.dimension === dimension,
                  )

                  return (
                    ranking && (
                      <RankingCard
                        key={dimension}
                        icon={icon}
                        title={title()}
                        ranking={ranking}
                        query={query}
                        metric={metric}
                        filters={filters}
                        labelOf={labelOf}
                        onFiltersChange={(next) =>
                          onSearchChange({ filters: next })
                        }
                      />
                    )
                  )
                })}
              </div>
            ))}
        </div>
      </ScrollArea>

      <TrafficToolbar
        search={search}
        retention={retention}
        paused={paused}
        labelOf={labelOf}
        onSearchChange={onSearchChange}
        onPausedChange={setPaused}
        onViewConnections={onViewConnections}
        start={toolbarStart}
      />
    </div>
  )
}
