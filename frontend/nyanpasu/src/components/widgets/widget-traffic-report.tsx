import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import RuleFolderRounded from '~icons/material-symbols/rule-folder-rounded'
import { motion, useReducedMotion } from 'motion/react'
import type { ComponentType, ReactNode } from 'react'
import { ActionSwap, ActionSwapText } from '@nyanpasu/ui/action-swap-text'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { LinearProgress } from '@nyanpasu/ui/progress'
import TextMarquee from '@nyanpasu/ui/text-marquee'
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
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetContext,
  useWidgetListCapacity,
  Widget,
  WidgetContent,
  WidgetHeader,
  WidgetMeta,
  WidgetMetric,
  WidgetTitle,
} from './core'
import {
  DEFAULT_WIDGET_CONFIGS,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetId,
  type ReportWidgetConfig,
} from './widget-config'
import {
  ConfigChoice,
  ConfigStepper,
  ConfigToggle,
} from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'
import { ReportProfileField } from './widget-reference-fields'
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
    <div className="text-on-surface-variant flex flex-1 items-center text-[length:var(--w-text-body)]">
      {children}
    </div>
  )
}

function ReportPreview({ kind }: { kind: 'recent' | ReportDimension }) {
  const dimension = kind === 'recent' ? null : kind
  const labels = dimension
    ? {
        origin: ['Safari', 'Chrome', 'Firefox'],
        exit: ['Japan Premium', 'Singapore Premium', 'DIRECT'],
        target: ['video.example.com', 'api.example.com', 'cdn.example.com'],
        rule: ['DOMAIN-SUFFIX,example.com', 'MATCH', 'GEOIP,CN'],
      }[dimension]
    : []
  const usages = [
    {
      bytes: { upload: 500_000_000, download: 1_200_000_000 },
      connections: 12,
    },
    { bytes: { upload: 134_003_200, download: 210_612_736 }, connections: 5 },
    { bytes: { upload: 100_000_000, download: 200_000_000 }, connections: 7 },
  ]
  const report: TrafficReport = {
    total: {
      bytes: { upload: 734_003_200, download: 1_610_612_736 },
      connections: 24,
    },
    current_rate: null,
    rankings: dimension
      ? [
          {
            dimension,
            distinct: 3,
            groups: labels.map((key, index) => ({
              key,
              usage: usages[index]!,
              current_rate: null,
            })),
            other: {
              bytes: { upload: 0, download: 0 },
              connections: 0,
            },
          },
        ]
      : [],
    topology: null,
  }

  return dimension ? (
    <RankingRows
      report={report}
      dimension={dimension}
      topN={3}
      showDirections={false}
      hideNames={false}
    />
  ) : (
    <RecentTrafficContent report={report} showDirections />
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
    <div className="bg-surface-variant/30 flex min-h-0 flex-1 flex-col justify-center gap-2 overflow-hidden rounded-[var(--w-radius-inner)] px-4 py-2">
      <WidgetMeta>{m.dashboard_widget_traffic_report_total_label()}</WidgetMeta>
      <WidgetMetric
        className="overflow-hidden text-[length:var(--w-text-metric)] whitespace-nowrap"
        data-slot="widget-traffic-report-total"
      >
        {formatBytes(total)}
      </WidgetMetric>
      {showDirections && (
        <WidgetMeta className="min-w-0">
          <TextMarquee className="w-full">
            <span className="mr-4">
              {m.dashboard_widget_traffic_report_upload({
                value: formatBytes(report.total.bytes.upload),
              })}
            </span>
            <span>
              {m.dashboard_widget_traffic_report_download({
                value: formatBytes(report.total.bytes.download),
              })}
            </span>
          </TextMarquee>
        </WidgetMeta>
      )}
      {total === 0 && (
        <WidgetMeta>
          {m.dashboard_widget_traffic_report_no_traffic()}
        </WidgetMeta>
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
  const { listRef, capacity } = useWidgetListCapacity({ baseGap: 0 })

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
      <EmptyState
        size="compact"
        tone="normal"
        icon={Icon}
        className="flex-1 overflow-hidden"
        data-slot={
          dimension === 'exit'
            ? 'widget-exit-traffic-empty'
            : 'widget-traffic-report-empty'
        }
        message={m.dashboard_widget_traffic_report_no_traffic()}
        description={
          dimension === 'exit'
            ? m.dashboard_widget_exit_traffic_empty_hint()
            : m.dashboard_widget_traffic_report_preview_ranking()
        }
      />
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

  const visibleCount = compact ? 0 : capacity
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
  const reducedMotion = useReducedMotion()
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
      <motion.div
        layout="position"
        initial={false}
        transition={{ duration: reducedMotion ? 0 : 0.22, ease: 'easeOut' }}
        className="flex h-[var(--w-row-height)] min-w-0 items-center gap-[var(--w-gap-sm)]"
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
          <div
            className="text-[length:var(--w-text-body)] font-medium"
            title={hideName ? undefined : label.title}
          >
            <TextMarquee className="w-full">{name}</TextMarquee>
          </div>
          <div className="text-on-surface-variant text-[length:var(--w-text-meta)] tabular-nums">
            <TextMarquee className="w-full">
              {m.dashboard_widget_traffic_report_share({
                percent: percent?.toFixed(1) ?? '0',
              })}
            </TextMarquee>
          </div>
        </div>
        <ActionSwapText
          className="shrink-0 text-[length:var(--w-text-body)] whitespace-nowrap tabular-nums"
          value={`${value} ${unit}`}
        />
      </motion.div>
    )
  }

  return (
    <div
      className={cn(
        'bg-surface-variant/30 text-on-surface flex flex-col rounded-[var(--w-radius-inner)]',
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
        <p className="text-[length:var(--w-text-meta)]">
          {dimension === 'exit'
            ? m.dashboard_widget_exit_traffic_leading()
            : m.dashboard_widget_traffic_report_leading()}
        </p>
      )}
      <ActionSwap
        className={cn(
          'min-w-0 font-medium',
          compact
            ? 'text-[length:var(--w-text-meta)]'
            : 'text-[length:var(--w-text-body)]',
        )}
        title={hideName ? undefined : label.title}
        contentKey={name}
      >
        <TextMarquee className="w-full">{name}</TextMarquee>
      </ActionSwap>
      <div className="flex items-baseline gap-2 tabular-nums">
        <ActionSwap
          contentKey={`${value} ${unit}`}
          className={cn(
            'min-w-0 flex-1 truncate whitespace-nowrap',
            compact
              ? 'text-[length:var(--w-text-body)]'
              : 'text-[length:var(--w-text-metric)]',
          )}
        >
          <span>{value}</span>{' '}
          <span className="text-[length:var(--w-text-meta)]">{unit}</span>
        </ActionSwap>
        <span className="shrink-0 text-[length:var(--w-text-body)] whitespace-nowrap">
          {share}
        </span>
      </div>
      <LinearProgress
        value={percent ?? 0}
        className={cn('bg-on-surface/10', compact ? 'h-1' : 'h-1.5')}
        aria-label={m.dashboard_widget_traffic_report_share({
          percent: percent?.toFixed(1) ?? '0',
        })}
      />
      {showDirections && (
        <div className="min-w-0 text-[length:var(--w-text-meta)] tabular-nums">
          <TextMarquee className="w-full">
            <span className="mr-4">
              {m.dashboard_widget_traffic_report_upload({
                value: formatBytes(group.usage.bytes.upload),
              })}
            </span>
            <span>
              {m.dashboard_widget_traffic_report_download({
                value: formatBytes(group.usage.bytes.download),
              })}
            </span>
          </TextMarquee>
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
    <div className="text-on-surface-variant flex shrink-0 items-center gap-2 text-[length:var(--w-text-meta)] tabular-nums">
      <span className="min-w-0 flex-1 truncate">
        {m.dashboard_widget_traffic_report_other({
          count: count.toLocaleString(),
        })}
      </span>
      <span className="shrink-0 whitespace-nowrap">
        {formatBytes(usageTotalBytes(usage))}
      </span>
      <span className="w-10 shrink-0 text-right whitespace-nowrap">
        {percent === null ? '—' : `${percent.toFixed(1)}%`}
      </span>
    </div>
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
  if (!result.data) {
    return (
      <TrafficStatus>
        {m.dashboard_widget_traffic_report_unavailable()}
      </TrafficStatus>
    )
  }

  return (
    <>
      {children(result as NonNullable<typeof result>)}
      {result.isError && (
        <p className="text-error text-[length:var(--w-text-meta)]">
          {m.dashboard_widget_traffic_report_refresh_error()}
        </p>
      )}
    </>
  )
}

function TrafficWidget({ dimension }: { dimension?: ReportDimension }) {
  const config = useWidgetConfig() as ReportWidgetConfig & {
    topN?: number
    hideNames?: boolean
  }
  const { span } = useWidgetContext()
  const profile = useDashboardTrafficProfile(config.profileUid)
  const compact = span.h <= 2
  const expanded = span.w > 3 || span.h > 2
  const showDirections =
    config.showDirections && !compact && (!dimension || expanded)
  const topN = config.topN ?? 3
  const hideNames = config.hideNames ?? false

  return (
    <Widget scaling="list" data-widget-kind="traffic-report">
      <WidgetHeader>
        <WidgetTitle />

        <span className="text-on-surface-variant shrink-0 text-[length:var(--w-text-meta)] whitespace-nowrap">
          {rangeLabels[config.range]()}
        </span>
      </WidgetHeader>

      <WidgetContent className="gap-2" data-slot="widget-traffic-report-card">
        {profile && (
          <WidgetMeta className="-mt-2 truncate">
            {profile.name
              ? `${m.dashboard_widget_config_profile()}: ${profile.name}`
              : profile.missing
                ? m.dashboard_widget_config_missing_reference({
                    name: profile.uid,
                  })
                : `${m.dashboard_widget_config_profile()}: ${profile.uid}`}
          </WidgetMeta>
        )}

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {dimension ? (
            <TrafficReportContent
              range={config.range}
              profileUid={config.profileUid}
            >
              {(result) => (
                <RankingRows
                  report={result.data!}
                  dimension={dimension}
                  topN={topN}
                  showDirections={showDirections}
                  hideNames={hideNames}
                  compact={compact}
                />
              )}
            </TrafficReportContent>
          ) : (
            <TrafficReportContent
              range={config.range}
              profileUid={config.profileUid}
            >
              {(result) => (
                <RecentTrafficContent
                  report={result.data!}
                  showDirections={showDirections}
                />
              )}
            </TrafficReportContent>
          )}
        </div>
      </WidgetContent>
    </Widget>
  )
}

function TrafficReportPreview({ kind }: { kind: 'recent' | ReportDimension }) {
  const config = useWidgetConfig() as ReportWidgetConfig
  const profile = useDashboardTrafficProfile(config.profileUid)

  return (
    <Widget scaling="list" data-widget-kind="traffic-report">
      <WidgetHeader>
        <WidgetTitle />

        <span className="text-on-surface-variant shrink-0 text-[length:var(--w-text-meta)] whitespace-nowrap">
          {rangeLabels[config.range]()}
        </span>
      </WidgetHeader>

      <WidgetContent className="gap-2" data-slot="widget-traffic-report-card">
        {profile && (
          <WidgetMeta className="-mt-2 truncate">
            {`${m.dashboard_widget_config_profile()}: ${profile.name ?? profile.uid}`}
          </WidgetMeta>
        )}

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <ReportPreview kind={kind} />
        </div>
      </WidgetContent>
    </Widget>
  )
}

function TrafficReportConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions()

  if (
    config.type !== WidgetId.RecentTraffic &&
    config.type !== WidgetId.OriginTraffic &&
    config.type !== WidgetId.ExitTraffic &&
    config.type !== WidgetId.TargetTraffic &&
    config.type !== WidgetId.RuleTraffic
  ) {
    return null
  }

  return (
    <>
      <ConfigGroup>
        <ConfigChoice
          label={m.dashboard_widget_config_range()}
          value={config.range}
          disabled={disabled}
          options={[
            {
              value: 'last_hour',
              label: m.dashboard_widget_config_range_hour(),
            },
            {
              value: 'last6_hours',
              label: m.dashboard_widget_config_range_6hours(),
            },
            {
              value: 'last24_hours',
              label: m.dashboard_widget_config_range_24hours(),
            },
            {
              value: 'last7_days',
              label: m.dashboard_widget_config_range_7days(),
            },
            {
              value: 'last30_days',
              label: m.dashboard_widget_config_range_30days(),
            },
            { value: 'all', label: m.dashboard_widget_config_range_all() },
          ]}
          onChange={(range) => updateConfig({ range })}
        />
        <ReportProfileField
          profileUid={config.profileUid}
          disabled={disabled}
          onChange={(profileUid) => updateConfig({ profileUid })}
        />
      </ConfigGroup>

      <ConfigGroup>
        <ConfigToggle
          label={m.dashboard_widget_config_directions()}
          checked={config.showDirections}
          disabled={disabled}
          onChange={(showDirections) => updateConfig({ showDirections })}
        />
        {config.type !== WidgetId.RecentTraffic && (
          <ConfigStepper
            label={m.dashboard_widget_config_top()}
            value={config.topN}
            range={WIDGET_CONFIG_NUMBER_RANGES.topN}
            disabled={disabled}
            onChange={(topN) => updateConfig({ topN })}
          />
        )}
        {(config.type === WidgetId.OriginTraffic ||
          config.type === WidgetId.TargetTraffic) && (
          <ConfigToggle
            label={m.dashboard_widget_config_hide_names()}
            checked={config.hideNames}
            disabled={disabled}
            onChange={(hideNames) => updateConfig({ hideNames })}
          />
        )}
      </ConfigGroup>
    </>
  )
}

function RecentTrafficLive() {
  return <TrafficWidget />
}

function RecentTrafficPreview() {
  return <TrafficReportPreview kind="recent" />
}

function OriginTrafficLive() {
  return <TrafficWidget dimension="origin" />
}

function OriginTrafficPreview() {
  return <TrafficReportPreview kind="origin" />
}

function ExitTrafficLive() {
  return <TrafficWidget dimension="exit" />
}

function ExitTrafficPreview() {
  return <TrafficReportPreview kind="exit" />
}

function TargetTrafficLive() {
  return <TrafficWidget dimension="target" />
}

function TargetTrafficPreview() {
  return <TrafficReportPreview kind="target" />
}

function RuleTrafficLive() {
  return <TrafficWidget dimension="rule" />
}

function RuleTrafficPreview() {
  return <TrafficReportPreview kind="rule" />
}

export const recentTrafficWidget = defineWidget({
  id: WidgetId.RecentTraffic,
  category: 'analytics',
  title: m.dashboard_widget_recent_traffic_title,
  icon: DataUsageRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.RecentTraffic],
  Component: RecentTrafficLive,
  Preview: RecentTrafficPreview,
  ConfigForm: TrafficReportConfigForm,
})

export const originTrafficWidget = defineWidget({
  id: WidgetId.OriginTraffic,
  category: 'analytics',
  title: m.dashboard_widget_origin_traffic_title,
  icon: AppsRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 4 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.OriginTraffic],
  Component: OriginTrafficLive,
  Preview: OriginTrafficPreview,
  ConfigForm: TrafficReportConfigForm,
})

export const exitTrafficWidget = defineWidget({
  id: WidgetId.ExitTraffic,
  category: 'analytics',
  title: m.dashboard_widget_exit_traffic_title,
  icon: CallSplitRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 4 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.ExitTraffic],
  Component: ExitTrafficLive,
  Preview: ExitTrafficPreview,
  ConfigForm: TrafficReportConfigForm,
})

export const targetTrafficWidget = defineWidget({
  id: WidgetId.TargetTraffic,
  category: 'analytics',
  title: m.dashboard_widget_target_traffic_title,
  icon: DnsRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 4 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.TargetTraffic],
  Component: TargetTrafficLive,
  Preview: TargetTrafficPreview,
  ConfigForm: TrafficReportConfigForm,
})

export const ruleTrafficWidget = defineWidget({
  id: WidgetId.RuleTraffic,
  category: 'analytics',
  title: m.dashboard_widget_rule_traffic_title,
  icon: RuleFolderRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 4 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.RuleTraffic],
  Component: RuleTrafficLive,
  Preview: RuleTrafficPreview,
  ConfigForm: TrafficReportConfigForm,
})
