import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import RuleFolderRounded from '~icons/material-symbols/rule-folder-rounded'
import { ComponentType, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { LinearProgress } from '@nyanpasu/ui/progress'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import { beyondRetention } from '@/utils/traffic-retention'
import { usageLabel } from '@/utils/traffic-usage'
import type {
  TrafficRange,
  TrafficReport,
  TrafficRetention,
  Usage,
  UsageGroup,
} from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId, type ReportWidgetConfig } from './widget-config'
import WidgetItem from './widget-item'
import {
  useDashboardTrafficProfile,
  useDashboardTrafficReport,
  useDashboardTrafficRetention,
} from './widget-traffic-provider'
import {
  addTrafficUsage,
  sliceTrafficRanking,
  trafficSharePercent,
  usageTotalBytes,
} from './widget-traffic-report-model'

const TRAFFIC_ROW_HEIGHT = 34
const TRAFFIC_ROW_HEIGHT_WITH_DIRECTIONS = 52

type ReportDimension = 'origin' | 'exit' | 'target' | 'rule'
type ReportIcon = ComponentType<{ className?: string }>

const rangeLabels: Record<TrafficRange, () => string> = {
  last_hour: m.dashboard_widget_traffic_report_range_last_hour,
  last6_hours: m.dashboard_widget_traffic_report_range_last6_hours,
  last24_hours: m.dashboard_widget_traffic_report_range_last24_hours,
  last7_days: m.dashboard_widget_traffic_report_range_last7_days,
  last30_days: m.dashboard_widget_traffic_report_range_last30_days,
  all: m.dashboard_widget_traffic_report_range_all,
}

const formatBytes = (bytes: number) => parseTraffic(bytes).join(' ')

const retentionLabels: Record<TrafficRetention, () => string> = {
  '1d': m.settings_nyanpasu_traffic_retention_1d,
  '7d': m.settings_nyanpasu_traffic_retention_7d,
  '30d': m.settings_nyanpasu_traffic_retention_30d,
  '90d': m.settings_nyanpasu_traffic_retention_90d,
  forever: m.settings_nyanpasu_traffic_retention_forever,
}

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
  retention,
  interactive,
  children,
}: WidgetComponentProps & {
  widgetType: WidgetId
  minW: number
  minH: number
  title: string
  icon: ReportIcon
  range: TrafficRange
  profile: { uid: string; name?: string; missing: boolean } | null
  retention: TrafficRetention | undefined
  interactive: boolean
  children: ReactNode
}) {
  const retentionLimited = beyondRetention(range, retention)
  const { sourceOnly, isOverlay } = useDndGridContext()
  const preview = sourceOnly || isOverlay

  return (
    <WidgetItem
      id={id}
      widgetType={widgetType}
      minW={minW}
      minH={minH}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-traffic-report-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2 p-4">
          <div className="flex min-w-0 items-center gap-2">
            <Icon className="text-on-surface-variant size-5 shrink-0" />
            <TextMarquee className="min-w-0 flex-1 font-medium">
              {title}
            </TextMarquee>
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
          <div className="text-on-surface-variant border-outline-variant flex shrink-0 items-center justify-between gap-2 border-t pt-2 text-xs">
            <div className="flex min-w-0 items-center">
              <span
                className={
                  retentionLimited ? 'text-error truncate' : 'truncate'
                }
              >
                {retention
                  ? m.dashboard_widget_traffic_report_retention({
                      value: retentionLabels[retention](),
                    })
                  : m.dashboard_widget_traffic_report_retention_unknown()}
                {retentionLimited &&
                  ` · ${m.dashboard_widget_traffic_report_retention_limited_short()}`}
              </span>
            </div>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="basic"
                  className="size-7 shrink-0"
                  icon
                  aria-label={m.dashboard_widget_traffic_report_open()}
                  asChild={!preview}
                  disabled={preview}
                >
                  {preview ? (
                    <OpenInNewRounded className="size-4" />
                  ) : (
                    <Link
                      aria-disabled={!interactive}
                      tabIndex={interactive ? 0 : -1}
                      className={
                        !interactive ? 'pointer-events-none opacity-50' : ''
                      }
                      onClick={(event) => {
                        if (!interactive) event.preventDefault()
                      }}
                      to="/main/topology"
                      search={{
                        range,
                        scope: 'all',
                        filters: [],
                        view: 'flow',
                        metric: 'bytes',
                        limit: 7,
                      }}
                      onPointerDown={(event) => event.stopPropagation()}
                    >
                      <OpenInNewRounded className="size-4" />
                    </Link>
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {m.dashboard_widget_traffic_report_open()}
              </TooltipContent>
            </Tooltip>
          </div>
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
    <div className="flex min-h-0 flex-1 flex-col justify-center gap-2">
      <p
        className="text-2xl font-bold tabular-nums"
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
}: {
  report: TrafficReport
  dimension: ReportDimension
  topN: 3 | 5
  showDirections: boolean
  hideNames: boolean
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
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_no_traffic()}
      </TrafficStatus>
    )
  }
  if (ranking.groups.length === 0) {
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_no_groups()}
      </TrafficStatus>
    )
  }

  const rowHeight = showDirections
    ? TRAFFIC_ROW_HEIGHT_WITH_DIRECTIONS
    : TRAFFIC_ROW_HEIGHT
  const visibleCount =
    listHeight == null
      ? 1
      : Math.max(1, Math.floor((listHeight + 8) / rowHeight))
  const visibleGroups = ranking.groups.slice(0, visibleCount)
  const otherUsage = ranking.groups
    .slice(visibleGroups.length)
    .reduce(
      (total, group) => addTrafficUsage(total, group.usage),
      ranking.other,
    )
  const otherCount = Math.max(0, ranking.distinct - visibleGroups.length)

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div ref={listRef} className="min-h-0 flex-1">
        <div className="flex flex-col gap-2">
          {visibleGroups.map((group, index) => (
            <TrafficRankingRow
              key={group.key}
              dimension={dimension}
              group={group}
              index={index}
              totalBytes={ranking.totalBytes}
              showDirections={showDirections}
              hideName={hideNames}
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
  index,
  totalBytes,
  showDirections,
  hideName,
}: {
  dimension: ReportDimension
  group: UsageGroup
  index: number
  totalBytes: number
  showDirections: boolean
  hideName: boolean
}) {
  const label = usageLabel(dimension, group.key)
  const percent = trafficSharePercent(group.usage, totalBytes)
  const visibleName = hideName
    ? m.dashboard_widget_traffic_report_hidden_name()
    : dimension === 'origin' && /[\\/]/.test(label.text)
      ? label.text.split(/[\\/]/).pop() || label.text
      : label.text
  const title = hideName
    ? undefined
    : dimension === 'origin'
      ? visibleName
      : label.title
  const total = usageTotalBytes(group.usage)

  return (
    <div className="min-w-0" data-slot="widget-traffic-report-row">
      <div className="flex min-w-0 items-center gap-2 text-xs">
        <span className="text-on-surface-variant w-4 shrink-0 tabular-nums">
          {index + 1}
        </span>
        <span className="min-w-0 flex-1 truncate" title={title}>
          {visibleName}
        </span>
        <span className="shrink-0 tabular-nums">{formatBytes(total)}</span>
        <span className="text-on-surface-variant w-10 shrink-0 text-right tabular-nums">
          {percent === null ? '—' : `${percent.toFixed(1)}%`}
        </span>
      </div>
      <LinearProgress
        value={percent ?? 0}
        aria-label={m.dashboard_widget_traffic_report_share({
          percent: percent?.toFixed(1) ?? '0',
        })}
        className="mt-1 h-1.5"
      />
      {showDirections && (
        <div className="text-on-surface-variant mt-1 flex gap-3 pl-6 text-[11px] tabular-nums">
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
    <div className="text-on-surface-variant border-outline-variant flex items-center gap-2 border-t pt-2 text-xs tabular-nums">
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
    topN?: 3 | 5
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
  const { sourceOnly, isOverlay, disabled, displayItems } = useDndGridContext()
  const retention = useDashboardTrafficRetention()
  const profile = useDashboardTrafficProfile(config.profileUid)
  const itemSize = displayItems.find((item) => item.id === id)
  const minH = dimension ? 3 : 2
  const preview = sourceOnly || isOverlay
  const expanded =
    itemSize !== undefined && (itemSize.w > 3 || itemSize.h > minH)
  const showDirections = config.showDirections && (!dimension || expanded)

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
      retention={retention}
      interactive={!preview && disabled}
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
