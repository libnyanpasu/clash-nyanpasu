import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowForwardRounded from '~icons/material-symbols/arrow-forward-rounded'
import { memo, type ReactNode } from 'react'
import HighlightText from '@nyanpasu/ui/highlight-text'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { CacheImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import type { Bytes, ClashRule } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { focusHighlightStyle } from '../../_modules/focus-highlight'
import type { RuleLiveStats } from './use-rule-stats'

export const RULE_SORTS = ['index', 'connections', 'speed', 'total'] as const

export type RuleSort = (typeof RULE_SORTS)[number]

export const RULE_ROW_HEIGHT = 56

// Shared by the header and the rows so their columns line up.
const GRID =
  'grid grid-cols-[2.5rem_minmax(0,1fr)_4rem_7rem_7rem] items-center gap-x-4 px-4'

const Placeholder = () => (
  <span className="text-on-surface-variant/40 text-right">—</span>
)

function HeaderCell({
  value,
  sort,
  onSortChange,
  end,
  children,
}: {
  value: RuleSort
  sort: RuleSort
  onSortChange: (sort: RuleSort) => void
  end?: boolean
  children: ReactNode
}) {
  // The rule order is the default, so it is not marked as a chosen sort.
  const active = sort === value && value !== 'index'

  return (
    <button
      type="button"
      className={cn(
        'flex h-full min-w-0 cursor-pointer items-center gap-1 select-none',
        'text-on-surface-variant hover:text-on-surface text-xs font-medium whitespace-nowrap',
        end && 'justify-end',
        active && 'text-primary hover:text-primary',
      )}
      // Selecting the sorted column again returns to the rule order.
      onClick={() => onSortChange(active ? 'index' : value)}
    >
      {active && <ArrowDownwardRounded className="size-3.5 shrink-0" />}

      <span className="truncate">{children}</span>
    </button>
  )
}

export function RulesHeader({
  sort,
  onSortChange,
}: {
  sort: RuleSort
  onSortChange: (sort: RuleSort) => void
}) {
  const cell = { sort, onSortChange }

  return (
    <div className={cn(GRID, 'h-9')} data-slot="rules-header">
      <HeaderCell value="index" end {...cell}>
        #
      </HeaderCell>

      <HeaderCell value="index" {...cell}>
        {m.rules_column_rule()}
      </HeaderCell>

      <HeaderCell value="connections" end {...cell}>
        {m.rules_column_connections()}
      </HeaderCell>

      <HeaderCell value="speed" end {...cell}>
        {m.rules_column_speed()}
      </HeaderCell>

      <HeaderCell value="total" end {...cell}>
        {m.rules_column_total()}
      </HeaderCell>
    </div>
  )
}

function Traffic({ value, rate }: { value: number; rate?: boolean }) {
  return (
    <span
      className={cn(
        value > 0 ? 'text-on-surface' : 'text-on-surface-variant/50',
      )}
    >
      {parseTraffic(value).join(' ')}
      {rate && '/s'}
    </span>
  )
}

/** A cell that jumps to the page showing what its number counts. */
function ViewButton({
  label,
  slot,
  onClick,
  children,
}: {
  label: string
  slot: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="hover:bg-primary/10 -mx-1 flex cursor-pointer rounded-md px-1"
          data-slot={slot}
          onClick={onClick}
        >
          {children}

          {/* The number stays in the name; the action follows it. */}
          <span className="sr-only">{label}</span>
        </button>
      </TooltipTrigger>

      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

/**
 * Download over upload, each prefixed with its direction. Phrasing content
 * only, as a jump button may hold it.
 */
function TrafficPair({
  download,
  upload,
  rate,
}: {
  download: number
  upload: number
  rate?: boolean
}) {
  return (
    <span className="flex flex-col items-end text-xs leading-5 tabular-nums">
      <span className="whitespace-nowrap">
        <span className="text-outline mr-1">↓</span>
        <Traffic value={download} rate={rate} />
      </span>

      <span className="whitespace-nowrap">
        <span className="text-outline mr-1">↑</span>
        <Traffic value={upload} rate={rate} />
      </span>
    </span>
  )
}

export const RuleRow = memo(function RuleRow({
  index,
  rule,
  live,
  total,
  label,
  icon,
  search,
  focused,
  onViewConnections,
  onViewUsage,
}: {
  /** 1-based position in the rule list. */
  index: number
  rule: ClashRule
  live?: RuleLiveStats
  /** Session totals; undefined while unknown. */
  total?: Bytes
  /** The rule's `ruleLabel`, passed back to the jump callbacks. */
  label: string
  /** The target group's icon. */
  icon?: string | null
  search: string
  /** Highlighted as the rule the page was opened or returned to. */
  focused?: boolean
  onViewConnections?: (label: string) => void
  onViewUsage?: (label: string) => void
}) {
  const active = !!live && live.connections > 0

  const hasTotal = !!total && total.download + total.upload > 0

  const badge = active && (
    <span
      className={cn(
        'min-w-6 rounded-full px-2 text-center text-xs leading-6 font-medium tabular-nums',
        'bg-primary-container text-on-primary-container',
      )}
    >
      {live.connections.toLocaleString()}
    </span>
  )

  const totalPair = hasTotal && (
    <TrafficPair download={total.download} upload={total.upload} />
  )

  return (
    <div
      className={cn(
        GRID,
        'border-outline-variant/25 border-b transition-colors',
        'hover:bg-primary/5',
      )}
      style={{
        height: RULE_ROW_HEIGHT,
        ...(focused && focusHighlightStyle),
      }}
      data-slot="rules-row"
      data-active={active}
      data-focused={focused}
    >
      <span
        className={cn(
          'text-right text-xs tabular-nums',
          active ? 'text-primary font-medium' : 'text-on-surface-variant/70',
        )}
      >
        {index}
      </span>

      <div className="min-w-0">
        <div className="text-on-surface truncate text-sm font-medium">
          <HighlightText searchText={search}>
            {rule.payload || rule.type}
          </HighlightText>
        </div>

        <div className="text-on-surface-variant mt-0.5 flex min-w-0 items-center gap-1.5 text-xs">
          {rule.payload && (
            <span
              className={cn(
                'shrink-0 rounded-md px-1.5 text-[11px] leading-4.5',
                'bg-secondary-container/70 text-on-secondary-container',
              )}
            >
              <HighlightText searchText={search}>{rule.type}</HighlightText>
            </span>
          )}

          <ArrowForwardRounded className="text-outline size-3.5 shrink-0" />

          {icon && (
            <CacheImage
              className="size-4 shrink-0"
              loadingClassName="rounded-full"
              icon={icon}
            />
          )}

          <span className="truncate">
            <HighlightText searchText={search}>{rule.proxy}</HighlightText>
          </span>
        </div>
      </div>

      <div className="flex justify-end">
        {active ? (
          onViewConnections ? (
            <ViewButton
              label={m.rules_view_connections()}
              slot="rules-row-view-connections"
              onClick={() => onViewConnections(label)}
            >
              {badge}
            </ViewButton>
          ) : (
            badge
          )
        ) : (
          <Placeholder />
        )}
      </div>

      <div className="flex justify-end">
        {active ? (
          <TrafficPair
            download={live.downloadSpeed}
            upload={live.uploadSpeed}
            rate
          />
        ) : (
          <Placeholder />
        )}
      </div>

      <div className="flex justify-end">
        {hasTotal ? (
          onViewUsage ? (
            <ViewButton
              label={m.rules_view_usage()}
              slot="rules-row-view-usage"
              onClick={() => onViewUsage(label)}
            >
              {totalPair}
            </ViewButton>
          ) : (
            totalPair
          )
        ) : (
          <Placeholder />
        )}
      </div>
    </div>
  )
})
