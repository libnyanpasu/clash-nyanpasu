import PauseRounded from '~icons/material-symbols/pause-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import { Button } from '@/components/ui/button'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import type {
  Dimension,
  TrafficRange,
  TrafficRetention,
  TrafficScope,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import FilterChip from './filter-chip'
import {
  beyondRetention,
  RANGES,
  type SearchFilter,
  type TrafficSearch,
} from './search'
import { dimensionName, type UsageLabel } from './usage-label'

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
  const { scope, range, filters } = search

  const removeFilter = (filter: SearchFilter) =>
    onSearchChange({ filters: filters.filter((item) => item !== filter) })

  return (
    <div
      className="bg-mixed-background flex min-h-16 shrink-0 flex-wrap items-center gap-x-2 gap-y-2 px-3 py-3 sm:flex-nowrap sm:gap-3 sm:px-4 sm:py-0"
      data-slot="traffic-toolbar"
    >
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
          // Below sm the chips take a row of their own under the controls.
          'order-last flex min-w-0 basis-full [scrollbar-width:none] items-center gap-2 overflow-x-auto',
          'sm:order-none sm:flex-1 sm:basis-0',
          filters.length === 0 && 'max-sm:hidden',
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

      <div className="w-32 shrink-0 sm:w-40">
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
  )
}
