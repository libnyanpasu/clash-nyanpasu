import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DevicesRounded from '~icons/material-symbols/devices-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import LoginRounded from '~icons/material-symbols/login-rounded'
import { useCallback, useMemo, useState } from 'react'
import { ScrollArea } from '@/components/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  useClashConnectionDetails,
  useClashWSStatus,
  useProfile,
  useSetting,
  useTrafficReport,
  type Dimension,
  type ReportRequest,
} from '@nyanpasu/interface'
import RankingCard from './ranking-card'
import { pollInterval, toQuery, type TrafficSearch } from './search'
import StatCards from './stat-cards'
import TopologyView from './topology-view'
import TrafficToolbar from './traffic-toolbar'
import { usageLabel } from './usage-label'

// One report serves the stat cards and every ranking card.
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

const Notice = ({ error, children }: { error?: boolean; children: string }) =>
  error ? (
    <p
      role="status"
      className="bg-error-container text-on-error-container rounded-2xl p-4 text-sm"
    >
      {children}
    </p>
  ) : (
    <p className="text-on-surface-variant py-6 text-center text-sm">
      {children}
    </p>
  )

export default function TrafficPage({
  search,
  onSearchChange,
}: {
  search: TrafficSearch
  onSearchChange: (update: Partial<TrafficSearch>) => void
}) {
  const { range, scope, filters, view, metric } = search

  const [paused, setPaused] = useState(false)

  const { value: retention } = useSetting('traffic_retention')

  const {
    query: { data: profiles },
  } = useProfile()

  const profileNames = useMemo(
    () =>
      profiles && new Map(profiles.items.map((item) => [item.uid, item.name])),
    [profiles],
  )

  const labelOf = useCallback(
    (dimension: Dimension, key: string) =>
      usageLabel(dimension, key, profileNames),
    [profileNames],
  )

  const query = useMemo(
    () => toQuery({ range, scope, filters }),
    [range, scope, filters],
  )

  const request = useMemo<ReportRequest>(
    () => ({
      query,
      rankings: RANKINGS.map(({ dimension }) => dimension),
      ranking_limit: RANKING_LIMIT,
      topology: null,
    }),
    [query],
  )

  const { data: report, isError } = useTrafficReport(request, {
    refetchInterval: paused ? false : pollInterval(range),
  })

  const { data: connections, isLoading } = useClashConnectionDetails()

  const { error } = useClashWSStatus()

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
            connections={connections?.connections ?? []}
            isLoading={isLoading}
            error={error}
            paused={paused}
            view={view}
            onViewChange={(next) => onSearchChange({ view: next })}
            metric={metric}
            onMetricChange={(next) => onSearchChange({ metric: next })}
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
      />
    </div>
  )
}
