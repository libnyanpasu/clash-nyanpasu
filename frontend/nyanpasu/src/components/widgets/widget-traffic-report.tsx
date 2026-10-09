import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import RuleFolderRounded from '~icons/material-symbols/rule-folder-rounded'
import { ComponentType, type ReactNode } from 'react'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { LinearProgress } from '@nyanpasu/ui/progress'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import { usageLabel } from '@/utils/traffic-usage'
import type {
  TrafficRange,
  TrafficReport,
  Usage,
  UsageGroup,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId, type ReportWidgetConfig } from './widget-config'
import WidgetItem from './widget-item'
import {
  useDashboardTrafficProfile,
  useDashboardTrafficReport,
} from './widget-traffic-provider'
import {
  addTrafficUsage,
  sliceTrafficRanking,
  trafficSharePercent,
  usageTotalBytes,
} from './widget-traffic-report-model'
import { WidgetTitle } from './widget-ui'

const TRAFFIC_ROW_HEIGHT = 48

type ReportDimension = 'origin' | 'exit' | 'target' | 'rule'
type ReportIcon = ComponentType<{ className?: string }>
const reportIcons: Record<ReportDimension, ReportIcon> = {
  origin: AppsRounded,
  exit: CallSplitRounded,
  target: DnsRounded,
  rule: RuleFolderRounded,
}

const rangeLabels: Record<TrafficRange, () => string> = {
  last_hour: m.dashboard_widget_traffic_report_range_last_hour,
  last6_hours: m.dashboard_widget_traffic_report_range_last6_hours,
  last24_hours: m.dashboard_widget_traffic_report_range_last24_hours,
  last7_days: m.dashboard_widget_traffic_report_range_last7_days,
  last30_days: m.dashboard_widget_traffic_report_range_last30_days,
  all: m.dashboard_widget_traffic_report_range_all,
}

const formatBytes = (bytes: number) => parseTraffic(bytes).join(' ')

function LoadingBody() {
  return (
    <div className="flex flex-1 flex-col justify-center gap-3" aria-busy="true">
      <div className="bg-surface-variant h-6 w-2/3 animate-pulse rounded" />
      <div className="bg-surface-variant h-3 w-full animate-pulse rounded" />
      <span className="sr-only">
        {m.dashboard_widget_traffic_report_loading()}
      </span>
    </div>
  )
}

function TrafficStatus({ children }: { children: string }) {
  return (
    <div className="text-on-surface-variant flex flex-1 items-center text-sm">
      {children}
    </div>
  )
}

function ReportWidgetShell({
  id,
  onCloseClick,
  widgetType,
  minW,
  minH,
  title,
  icon: Icon,
  range,
  profile,

  children,
}: WidgetComponentProps & {
  widgetType: WidgetId
  minW: number
  minH: number
  title: string
  icon: ReportIcon
  range: TrafficRange
  profile: { uid: string; name?: string; missing: boolean } | null

  children: ReactNode
}) {
  return (
    <WidgetItem
      id={id}
      widgetType={widgetType}
      minW={minW}
      minH={minH}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-traffic-report-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2 overflow-hidden p-4">
          <div className="flex min-w-0 items-center gap-2">
            <WidgetTitle className="flex-1" icon={Icon}>
              {title}
            </WidgetTitle>
            <span className="text-on-surface-variant shrink-0 text-xs">
              {rangeLabels[range]()}
            </span>
          </div>
          {profile && (
            <p className="text-on-surface-variant -mt-2 truncate text-xs">
              {profile.name
                ? `${m.dashboard_widget_config_profile()}: ${profile.name}`
                : profile.missing
                  ? m.dashboard_widget_config_missing_reference({
                      name: profile.uid,
                    })
                  : `${m.dashboard_widget_config_profile()}: ${profile.uid}`}
            </p>
          )}
          {children}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function TrafficReportContent({
  range,
  profileUid,
  children,
}: {
  range: TrafficRange
  profileUid: string | null
  children: (
    report: NonNullable<ReturnType<typeof useDashboardTrafficReport>>,
  ) => ReactNode
}) {
  const result = useDashboardTrafficReport(range, profileUid)
  if (!result) {
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_unavailable()}
      </TrafficStatus>
    )
  }
  if (!result.data && result.isPending) return <LoadingBody />
  if (!result.data && result.isError) {
    return (
      <TrafficStatus>{m.dashboard_widget_traffic_report_error()}</TrafficStatus>
    )
  }
  if (!result.data)
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_unavailable()}
      </TrafficStatus>
    )

  return (
    <>
      {children(result as NonNullable<typeof result>)}
      {result.isError && (
        <p className="text-error text-xs">
          {m.dashboard_widget_traffic_report_refresh_error()}
        </p>
      )}
    </>
  )
}

function ReportPreview({ kind }: { kind: 'recent' | ReportDimension }) {
  return (
    <TrafficStatus>
      {kind === 'recent'
        ? m.dashboard_widget_traffic_report_preview()
        : m.dashboard_widget_traffic_report_preview_ranking()}
    </TrafficStatus>
  )
}

function RecentTrafficContent({
  report,
  showDirections,
}: {
  report: TrafficReport
  showDirections: boolean
}) {
  const total = usageTotalBytes(report.total)

  return (
    <div className="bg-surface-variant/30 flex min-h-0 flex-1 flex-col justify-center gap-2 overflow-hidden rounded-2xl px-4 py-2">
      <p className="text-on-surface-variant text-xs">
        {m.dashboard_widget_traffic_report_total_label()}
      </p>
      <p
        className="text-3xl tabular-nums"
        data-slot="widget-traffic-report-total"
      >
        {formatBytes(total)}
      </p>
      {showDirections && (
        <div className="text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs tabular-nums">
          <span>
            {m.dashboard_widget_traffic_report_upload({
              value: formatBytes(report.total.bytes.upload),
            })}
          </span>
          <span>
            {m.dashboard_widget_traffic_report_download({
              value: formatBytes(report.total.bytes.download),
            })}
          </span>
        </div>
      )}
      {total === 0 && (
        <p className="text-on-surface-variant text-xs">
          {m.dashboard_widget_traffic_report_no_traffic()}
        </p>
      )}
    </div>
  )
}

function RankingRows({
  report,
  dimension,
  topN,
  showDirections,
  hideNames,
  compact = false,
}: {
  report: TrafficReport
  dimension: ReportDimension
  topN: number
  showDirections: boolean
  hideNames: boolean
  compact?: boolean
}) {
  const ranking = sliceTrafficRanking(report, dimension, topN)
  const { ref: listRef, height: listHeight } = useWidgetHeight<HTMLDivElement>()
  if (!ranking) {
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_unavailable()}
      </TrafficStatus>
    )
  }
  if (ranking.totalBytes === 0) {
    const Icon = reportIcons[dimension]
    return (
      <div
        className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-hidden text-center"
        data-slot={
          dimension === 'exit'
            ? 'widget-exit-traffic-empty'
            : 'widget-traffic-report-empty'
        }
      >
        <span
          className="bg-secondary-container text-on-secondary-container flex size-14 shrink-0 items-center justify-center rounded-2xl"
          aria-hidden="true"
        >
          <Icon className="size-7" />
        </span>
        <div>
          <p className="text-base font-medium">
            {m.dashboard_widget_traffic_report_no_traffic()}
          </p>
          <p className="text-on-surface-variant mt-1 text-xs">
            {dimension === 'exit'
              ? m.dashboard_widget_exit_traffic_empty_hint()
              : m.dashboard_widget_traffic_report_preview_ranking()}
          </p>
        </div>
      </div>
    )
  }
  if (ranking.groups.length === 0) {
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_no_groups()}
      </TrafficStatus>
    )
  }

  const featured = ranking.groups[0]
  if (featured && ranking.distinct === 1 && ranking.otherCount === 0) {
    return (
      <TrafficRankingRow
        dimension={dimension}
        group={featured}
        totalBytes={ranking.totalBytes}
        hideName={hideNames}
        featured
        expanded
        compact={compact}
        showDirections={showDirections}
      />
    )
  }

  const visibleCount =
    compact || listHeight == null
      ? 0
      : Math.max(0, Math.floor(listHeight / TRAFFIC_ROW_HEIGHT))
  const visibleGroups = ranking.groups.slice(1, visibleCount + 1)
  const displayedCount = visibleGroups.length + 1
  const otherUsage = ranking.groups
    .slice(displayedCount)
    .reduce(
      (total, group) => addTrafficUsage(total, group.usage),
      ranking.other,
    )
  const otherCount = Math.max(0, ranking.distinct - displayedCount)

  return (
    <div
      className={cn(
        'flex min-h-0 flex-1 flex-col overflow-hidden',
        compact ? 'gap-0.5' : 'gap-2',
      )}
    >
      {featured && (
        <TrafficRankingRow
          dimension={dimension}
          group={featured}
          totalBytes={ranking.totalBytes}
          hideName={hideNames}
          featured
          compact={compact}
          showDirections={showDirections}
        />
      )}
      <div ref={listRef} className="min-h-0 flex-1">
        <div className="flex flex-col">
          {visibleGroups.map((group) => (
            <TrafficRankingRow
              key={group.key}
              group={group}
              dimension={dimension}
              totalBytes={ranking.totalBytes}
              hideName={hideNames}
              featured={false}
              showDirections={false}
            />
          ))}
        </div>
      </div>
      {otherCount > 0 && (
        <TrafficOtherRow
          usage={otherUsage}
          count={otherCount}
          totalBytes={ranking.totalBytes}
        />
      )}
    </div>
  )
}

function TrafficRankingRow({
  dimension,
  group,
  totalBytes,
  hideName,
  featured,
  showDirections,
  expanded = false,
  compact = false,
}: {
  dimension: ReportDimension
  group: UsageGroup
  totalBytes: number
  hideName: boolean
  featured: boolean
  showDirections: boolean
  expanded?: boolean
  compact?: boolean
}) {
  const Icon = reportIcons[dimension]
  const label = usageLabel(dimension, group.key)
  const name = hideName
    ? m.dashboard_widget_traffic_report_hidden_name()
    : dimension === 'origin'
      ? label.text.split(/[\\/]/).pop() || label.text
      : label.text
  const percent = trafficSharePercent(group.usage, totalBytes)
  const share = percent === null ? '—' : `${percent.toFixed(1)}%`
  const [value, unit] = parseTraffic(usageTotalBytes(group.usage))

  if (!featured) {
    return (
      <div
        className="flex h-12 min-w-0 items-center gap-3"
        data-slot={
          dimension === 'exit'
            ? 'widget-exit-traffic-row'
            : 'widget-traffic-report-row'
        }
      >
        <span
          className="bg-surface-variant text-on-surface-variant flex size-8 shrink-0 items-center justify-center rounded-full"
          aria-hidden="true"
        >
          <Icon className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <p
            className="truncate text-sm font-medium"
            title={hideName ? undefined : label.title}
          >
            {name}
          </p>
          <p className="text-on-surface-variant text-xs tabular-nums">
            {m.dashboard_widget_traffic_report_share({
              percent: percent?.toFixed(1) ?? '0',
            })}
          </p>
        </div>
        <span className="shrink-0 text-sm tabular-nums">
          {value} {unit}
        </span>
      </div>
    )
  }

  return (
    <div
      className={cn(
        'bg-surface-variant/30 text-on-surface flex flex-col rounded-2xl',
        compact ? 'gap-0.5 px-3 py-1' : 'gap-1 px-4 py-3',
        expanded ? 'min-h-0 flex-1 justify-between' : 'shrink-0',
      )}
      data-slot={
        dimension === 'exit'
          ? 'widget-exit-traffic-featured'
          : 'widget-traffic-report-featured'
      }
    >
      {!compact && (
        <p className="text-xs">
          {dimension === 'exit'
            ? m.dashboard_widget_exit_traffic_leading()
            : m.dashboard_widget_traffic_report_leading()}
        </p>
      )}
      <p
        className={cn(
          'truncate font-medium',
          compact ? 'text-xs' : 'text-base',
        )}
        title={hideName ? undefined : label.title}
      >
        {name}
      </p>
      <div className="flex items-baseline gap-2 tabular-nums">
        <p
          className={cn(
            'min-w-0 flex-1 truncate',
            compact ? 'text-xl' : 'text-3xl',
          )}
        >
          <span>{value}</span> <span className="text-sm">{unit}</span>
        </p>
        <span className="shrink-0 text-base">{share}</span>
      </div>
      <LinearProgress
        value={percent ?? 0}
        className={cn('bg-on-surface/10', compact ? 'h-1' : 'h-1.5')}
        aria-label={m.dashboard_widget_traffic_report_share({
          percent: percent?.toFixed(1) ?? '0',
        })}
      />
      {showDirections && (
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs tabular-nums">
          <span>
            {m.dashboard_widget_traffic_report_upload({
              value: formatBytes(group.usage.bytes.upload),
            })}
          </span>
          <span>
            {m.dashboard_widget_traffic_report_download({
              value: formatBytes(group.usage.bytes.download),
            })}
          </span>
        </div>
      )}
    </div>
  )
}

function TrafficOtherRow({
  usage,
  count,
  totalBytes,
}: {
  usage: Usage
  count: number
  totalBytes: number
}) {
  const percent = trafficSharePercent(usage, totalBytes)

  return (
    <div className="text-on-surface-variant flex shrink-0 items-center gap-2 text-xs tabular-nums">
      <span className="min-w-0 flex-1 truncate">
        {m.dashboard_widget_traffic_report_other({
          count: count.toLocaleString(),
        })}
      </span>
      <span>{formatBytes(usageTotalBytes(usage))}</span>
      <span className="w-10 text-right">
        {percent === null ? '—' : `${percent.toFixed(1)}%`}
      </span>
    </div>
  )
}

function useReportConfig(id: string, widgetType: WidgetId) {
  return useWidgetConfig(id, widgetType) as ReportWidgetConfig & {
    topN?: number
    hideNames?: boolean
  }
}

function TrafficWidget({
  id,
  onCloseClick,
  widgetType,
  title,
  icon,
  dimension,
}: WidgetComponentProps & {
  widgetType: WidgetId
  title: string
  icon: ReportIcon
  dimension?: ReportDimension
}) {
  const config = useReportConfig(id, widgetType)
  const { sourceOnly, isOverlay, displayItems } = useDndGridContext()

  const profile = useDashboardTrafficProfile(config.profileUid)
  const itemSize = displayItems.find((item) => item.id === id)
  const minH = 2
  const compact = itemSize !== undefined && itemSize.h <= 2
  const preview = sourceOnly || isOverlay
  const expanded =
    itemSize !== undefined && (itemSize.w > 3 || itemSize.h > minH)
  const showDirections =
    config.showDirections && !compact && (!dimension || expanded)

  return (
    <ReportWidgetShell
      id={id}
      onCloseClick={onCloseClick}
      widgetType={widgetType}
      minW={3}
      minH={minH}
      title={title}
      icon={icon}
      range={config.range}
      profile={profile}
    >
      {preview ? (
        <ReportPreview kind={dimension ?? 'recent'} />
      ) : (
        <TrafficReportContent
          range={config.range}
          profileUid={config.profileUid}
        >
          {(result) =>
            dimension ? (
              <RankingRows
                report={result.data!}
                dimension={dimension}
                topN={config.topN ?? 3}
                showDirections={showDirections}
                hideNames={config.hideNames ?? false}
                compact={compact}
              />
            ) : (
              <RecentTrafficContent
                report={result.data!}
                showDirections={showDirections}
              />
            )
          }
        </TrafficReportContent>
      )}
    </ReportWidgetShell>
  )
}

export function RecentTrafficWidget(props: WidgetComponentProps) {
  return (
    <TrafficWidget
      {...props}
      widgetType={WidgetId.RecentTraffic}
      title={m.dashboard_widget_recent_traffic_title()}
      icon={DataUsageRounded}
    />
  )
}

export function OriginTrafficWidget(props: WidgetComponentProps) {
  return (
    <TrafficWidget
      {...props}
      widgetType={WidgetId.OriginTraffic}
      title={m.dashboard_widget_origin_traffic_title()}
      icon={AppsRounded}
      dimension="origin"
    />
  )
}

export function ExitTrafficWidget(props: WidgetComponentProps) {
  return (
    <TrafficWidget
      {...props}
      widgetType={WidgetId.ExitTraffic}
      title={m.dashboard_widget_exit_traffic_title()}
      icon={CallSplitRounded}
      dimension="exit"
    />
  )
}

export function TargetTrafficWidget(props: WidgetComponentProps) {
  return (
    <TrafficWidget
      {...props}
      widgetType={WidgetId.TargetTraffic}
      title={m.dashboard_widget_target_traffic_title()}
      icon={DnsRounded}
      dimension="target"
    />
  )
}

export function RuleTrafficWidget(props: WidgetComponentProps) {
  return (
    <TrafficWidget
      {...props}
      widgetType={WidgetId.RuleTraffic}
      title={m.dashboard_widget_rule_traffic_title()}
      icon={RuleFolderRounded}
      dimension="rule"
    />
  )
}
