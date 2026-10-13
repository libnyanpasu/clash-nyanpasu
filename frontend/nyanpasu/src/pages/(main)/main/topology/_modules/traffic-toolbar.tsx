import LanOutlineRounded from '~icons/material-symbols/lan-outline-rounded'
import PauseRounded from '~icons/material-symbols/pause-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import type { PropsWithChildren } from 'react'
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
import { dimensionName } from '@/utils/traffic-usage'
import type { Metric, TrafficRange, TrafficScope } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  dataPageFloatingGroupVariants,
  dataPageTabsVariants,
  DataPageToolbar,
} from '../../_modules/data-page'
import FilterChips from '../../_modules/filter-chips'
import {
  rangeName,
  RANGES,
  type SearchFilter,
} from '../../_modules/traffic-filters'
import { useTrafficPage } from './traffic-context'

const SCOPES = {
  all: m.traffic_scope_all,
  active: m.traffic_scope_active,
  closed: m.traffic_scope_closed,
} satisfies Record<TrafficScope, () => string>

const METRICS = {
  bytes: m.traffic_metric_bytes,
  connections: m.traffic_metric_connections,
} satisfies Record<Metric, () => string>

export default function TrafficToolbar({ children }: PropsWithChildren) {
  const {
    search,
    retention,
    paused,
    labelOf,
    onSearchChange,
    onPausedChange,
    onViewConnections,
  } = useTrafficPage()
  const { scope, metric, range, filters } = search

  const removeFilter = (filter: SearchFilter) =>
    onSearchChange({ filters: filters.filter((item) => item !== filter) })

  return (
    <DataPageToolbar className="@container" data-slot="traffic-toolbar">
      {children}

      <div
        className={dataPageFloatingGroupVariants()}
        data-slot="traffic-scope"
      >
        <SegmentedButton
          size="sm"
          variant="tabs"
          className={dataPageTabsVariants()}
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
              className="flex-none rounded-full whitespace-nowrap"
            >
              {SCOPES[value]()}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>
      </div>

      <div
        className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-2"
        data-slot="traffic-controls"
      >
        <div
          className={cn(
            dataPageFloatingGroupVariants(),
            'max-w-full flex-wrap justify-end',
          )}
          data-slot="traffic-metric-range"
        >
          <div className="w-28 shrink-0">
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
                className="h-9 min-w-0 rounded-full px-3 py-0"
                aria-label={m.traffic_metric_label()}
                title={METRICS[metric]()}
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

          <div className="w-32 shrink-0">
            <Select
              variant="outlined"
              value={range}
              onValueChange={(next) =>
                onSearchChange({ range: next as TrafficRange })
              }
            >
              <SelectTrigger
                className="h-9 min-w-0 rounded-full px-3 py-0"
                aria-label={m.traffic_range_label()}
                title={rangeName(range)}
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
        </div>

        <div
          className={dataPageFloatingGroupVariants()}
          data-slot="traffic-actions"
        >
          {onViewConnections && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  icon
                  className="rounded-full"
                  aria-label={m.traffic_view_connections()}
                  data-slot="traffic-view-connections"
                  onClick={onViewConnections}
                >
                  <LanOutlineRounded />
                </Button>
              </TooltipTrigger>

              <TooltipContent>{m.traffic_view_connections()}</TooltipContent>
            </Tooltip>
          )}

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                icon
                className="rounded-full"
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

      {filters.length > 0 && (
        <FilterChips
          className={cn(
            dataPageFloatingGroupVariants(),
            'order-last min-h-11 basis-full',
          )}
          data-slot="traffic-filters"
          items={filters.map((filter) => {
            const label = labelOf(filter.d, filter.v)

            return {
              key: filter.d,
              label: `${dimensionName(filter.d)}: ${label.text}`,
              title: label.title,
              onRemove: () => removeFilter(filter),
            }
          })}
          onClearAll={() => onSearchChange({ filters: [] })}
        />
      )}
    </DataPageToolbar>
  )
}
