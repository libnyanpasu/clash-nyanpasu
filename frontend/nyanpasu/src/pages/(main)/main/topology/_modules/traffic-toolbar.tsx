import PauseRounded from '~icons/material-symbols/pause-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { beyondRetention } from '@/utils/traffic-retention'
import { dimensionName, type UsageLabel } from '@/utils/traffic-usage'
import type {
  Dimension,
  Metric,
  TrafficRange,
  TrafficRetention,
  TrafficScope,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import FilterChip from './filter-chip'
import { RANGES, type SearchFilter, type TrafficSearch } from './search'

const rangeName = (range: TrafficRange) =>
  ({
    last_hour: m.traffic_range_last_hour,
    last6_hours: m.traffic_range_last6_hours,
    last24_hours: m.traffic_range_last24_hours,
    last7_days: m.traffic_range_last7_days,
    last30_days: m.traffic_range_last30_days,
    all: m.traffic_range_all,
  })[range]()

const SCOPES = {
  all: m.traffic_scope_all,
  active: m.traffic_scope_active,
  closed: m.traffic_scope_closed,
} satisfies Record<TrafficScope, () => string>

const METRICS = {
  bytes: m.traffic_metric_bytes,
  connections: m.traffic_metric_connections,
} satisfies Record<Metric, () => string>

export default function TrafficToolbar({
  search,
  retention,
  paused,
  labelOf,
  onSearchChange,
  onPausedChange,
}: {
  search: TrafficSearch
  retention?: TrafficRetention
  paused: boolean
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSearchChange: (update: Partial<TrafficSearch>) => void
  onPausedChange: (paused: boolean) => void
}) {
  const { scope, metric, range, filters } = search

  const removeFilter = (filter: SearchFilter) =>
    onSearchChange({ filters: filters.filter((item) => item !== filter) })

  // It goes on one line once it is wide enough for every control, whatever the
  // window: the navigation beside the page takes part of the viewport.
  return (
    <div
      className="bg-mixed-background @container shrink-0"
      data-slot="traffic-toolbar"
    >
      <div className="flex min-h-16 flex-wrap items-center gap-x-2 gap-y-2 px-3 py-3 @2xl:flex-nowrap @2xl:gap-3 @2xl:px-4 @2xl:py-0">
        <SegmentedButton
          size="sm"
          className="w-auto shrink-0"
          aria-label={m.traffic_scope_label()}
          value={scope}
          onValueChange={(next) => {
            // Selecting the selected segment again clears a toggle group.
            if (next === 'all' || next === 'active' || next === 'closed') {
              onSearchChange({ scope: next })
            }
          }}
        >
          {(Object.keys(SCOPES) as TrafficScope[]).map((value) => (
            <SegmentedButtonItem
              key={value}
              value={value}
              className="flex-none whitespace-nowrap"
            >
              {SCOPES[value]()}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>

        <div
          className={cn(
            // When it wraps, the chips take a row of their own under the controls.
            'order-last flex min-w-0 basis-full [scrollbar-width:none] items-center gap-2 overflow-x-auto',
            '@2xl:order-none @2xl:flex-1 @2xl:basis-0',
            filters.length === 0 && '@max-2xl:hidden',
          )}
          role="group"
          aria-label={m.traffic_filters_label()}
        >
          {filters.map((filter) => {
            const label = labelOf(filter.d, filter.v)

            return (
              <FilterChip
                key={filter.d}
                label={`${dimensionName(filter.d)}: ${label.text}`}
                title={label.title}
                onRemove={() => removeFilter(filter)}
              />
            )
          })}

          {filters.length > 0 && (
            <Button
              className="h-8 shrink-0 px-3 whitespace-nowrap"
              onClick={() => onSearchChange({ filters: [] })}
            >
              {m.traffic_filter_clear_all()}
            </Button>
          )}
        </div>

        {/* What the rankings, the flow and the map all weigh usage by: a view
            setting like the time range, beside it, while the segmented button
            keeps the scope, as on the connections page. When the bar wraps,
            the two settings share the second row and the pause button stays
            beside the scope. */}
        <div className="w-28 shrink-0 @max-2xl:order-1 @2xl:w-36">
          <Select
            variant="outlined"
            value={metric}
            onValueChange={(next) => {
              if (next === 'bytes' || next === 'connections') {
                onSearchChange({ metric: next })
              }
            }}
          >
            <SelectTrigger
              className="h-10 min-w-0 py-2"
              aria-label={m.traffic_metric_label()}
            >
              <SelectValue
                className="truncate pr-4 text-sm"
                placeholder={m.traffic_metric_label()}
              >
                {METRICS[metric]()}
              </SelectValue>
            </SelectTrigger>

            <SelectContent>
              {(Object.keys(METRICS) as Metric[]).map((value) => (
                <SelectItem key={value} value={value}>
                  {METRICS[value]()}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="w-32 shrink-0 @max-2xl:order-1 @2xl:w-40">
          <Select
            variant="outlined"
            value={range}
            onValueChange={(next) =>
              onSearchChange({ range: next as TrafficRange })
            }
          >
            <SelectTrigger
              className="h-10 min-w-0 py-2"
              aria-label={m.traffic_range_label()}
            >
              <SelectValue
                className="truncate pr-4 text-sm"
                placeholder={m.traffic_range_label()}
              >
                {rangeName(range)}
              </SelectValue>
            </SelectTrigger>

            <SelectContent className="min-w-64">
              {RANGES.map((value) => {
                const disabled = beyondRetention(value, retention)

                return (
                  <SelectItem
                    key={value}
                    value={value}
                    disabled={disabled}
                    className="data-disabled:pointer-events-none data-disabled:opacity-50"
                  >
                    <span>{rangeName(value)}</span>

                    {disabled && (
                      <span className="text-on-surface-variant text-xs">
                        {m.traffic_range_beyond_retention()}
                      </span>
                    )}
                  </SelectItem>
                )
              })}
            </SelectContent>
          </Select>
        </div>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              icon
              aria-label={paused ? m.topology_resume() : m.topology_pause()}
              aria-pressed={paused}
              onClick={() => onPausedChange(!paused)}
            >
              {paused ? <PlayArrowRounded /> : <PauseRounded />}
            </Button>
          </TooltipTrigger>

          <TooltipContent>
            {paused ? m.topology_resume() : m.topology_pause()}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
