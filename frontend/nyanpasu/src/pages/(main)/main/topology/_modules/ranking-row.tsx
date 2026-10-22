import { motion } from 'motion/react'
import type { ReactNode } from 'react'
import {
  SharedTooltip,
  useSharedTooltipTrigger,
} from '@nyanpasu/ui/shared-tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import { usageAmount, type UsageLabel } from '@/utils/traffic-usage'
import type { Metric, Usage } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { usePageTransition } from './transition'

const traffic = (bytes: number) => parseTraffic(bytes).join(' ')

/** The tooltip the ranking rows inside it share, for their full labels. */
export function RankingRowTooltip({ children }: { children: ReactNode }) {
  return (
    <SharedTooltip className="max-w-96 rounded-2xl break-all">
      {children}
    </SharedTooltip>
  )
}

/**
 * The height of a card row: its padding and its two lines, one of text-sm and
 * one of text-xs, a gap apart. Each line is truncated, so it never wraps, and
 * keeps its line height when a glyph comes from a fallback font with taller
 * metrics.
 */
export const CARD_ROW_HEIGHT = '3.375rem'

/**
 * A group of a ranking. Inside `AnimatePresence`, an animated row fades in
 * and slides to a new place. A leaving row goes at once: when a filter
 * replaces the whole list, animating every row out stalls the page. Its full
 * label shows in the enclosing `SharedTooltip`, on hover and on keyboard
 * focus.
 */
export default function RankingRow({
  label,
  usage,
  metric,
  rank,
  active,
  onSelect,
  layout = 'card',
  animated = true,
}: {
  label: UsageLabel
  usage: Usage
  /** The amount the row leads with; the other one closes its second line. */
  metric: Metric
  /** Its place in the list, 0 for the heaviest group; null below the shown places. */
  rank: number | null
  /** Whether the group is already a filter. */
  active: boolean
  onSelect: () => void
  /** How the metrics are arranged within the row. */
  layout?: 'card' | 'table'
  /**
   * Whether the row fades in and follows its place. A list that is not
   * refreshed has nothing to animate, and a long one pays for it per row.
   */
  animated?: boolean
}) {
  const { transition } = usePageTransition()
  const tooltip = useSharedTooltipTrigger(label.title)

  const { upload, download } = usage.bytes

  const first = rank === 0
  const total = usageAmount(usage, 'bytes')
  const connections = usageAmount(usage, 'connections')
  const secondary = metric === 'bytes' ? connections : total
  const primary = usageAmount(usage, metric)
  const tableSecondary =
    metric === 'bytes' ? usage.connections.toLocaleString() : total
  const tablePrimary =
    metric === 'bytes' ? total : usage.connections.toLocaleString()
  const secondaryLabel =
    metric === 'bytes' ? m.traffic_stat_connections() : m.traffic_stat_total()

  const props = {
    type: 'button' as const,
    'aria-pressed': active,
    className: cn(
      layout === 'table'
        ? 'grid w-full min-w-0 cursor-pointer grid-cols-[minmax(0,2fr)_repeat(4,minmax(0,1fr))] items-center gap-2 rounded-xl px-2 py-2 text-left outline-none @max-lg:grid-cols-[minmax(0,1fr)_auto] @max-lg:grid-rows-[auto_auto] @max-lg:gap-x-3 @max-lg:gap-y-1'
        : 'flex w-full min-w-0 cursor-pointer flex-col gap-0.5 rounded-xl px-3 py-2 text-left outline-none',
      'focus-visible:ring-primary focus-visible:ring-2',
      'hover:bg-on-surface/8 transition-colors',
      first && 'bg-secondary-container/60',
      active && 'bg-secondary-container',
    ),
    ...tooltip,
    onClick: () => {
      onSelect()
      tooltip.onClick()
    },
  }

  const content =
    layout === 'table' ? (
      <>
        <span className="flex min-w-0 items-baseline gap-1.5 @max-lg:col-start-1 @max-lg:row-start-1">
          {rank !== null && (
            <span className="text-on-surface-variant shrink-0 text-[10px] tabular-nums">
              {String(rank + 1).padStart(2, '0')}
            </span>
          )}

          <span
            className={cn(
              'min-w-0 truncate text-sm font-medium',
              label.mono && 'font-mono',
            )}
          >
            {label.text}
          </span>
        </span>

        <span
          className="text-on-surface-variant hidden min-w-0 text-right text-xs whitespace-nowrap tabular-nums @lg:block"
          title={traffic(upload)}
        >
          {traffic(upload)}
        </span>

        <span
          className="text-on-surface-variant hidden min-w-0 text-right text-xs whitespace-nowrap tabular-nums @lg:block"
          title={traffic(download)}
        >
          {traffic(download)}
        </span>

        <span
          className="text-on-surface-variant hidden min-w-0 text-right text-xs whitespace-nowrap tabular-nums @lg:block"
          title={tableSecondary}
        >
          {tableSecondary}
        </span>

        <span
          className={cn(
            'min-w-0 text-right text-[13px] font-medium whitespace-nowrap tabular-nums @max-lg:col-start-2 @max-lg:row-start-1',
            first && 'text-primary font-medium',
          )}
          title={tablePrimary}
        >
          {tablePrimary}
        </span>

        <span className="text-on-surface-variant col-span-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs tabular-nums @lg:hidden">
          <span className="whitespace-nowrap">
            <span className="text-outline mr-1">↑</span>
            <span className="text-on-surface-variant mr-1">
              {m.connections_field_upload()}
            </span>
            {traffic(upload)}
          </span>

          <span className="whitespace-nowrap">
            <span className="text-outline mr-1">↓</span>
            <span className="text-on-surface-variant mr-1">
              {m.connections_field_download()}
            </span>
            {traffic(download)}
          </span>

          <span className="whitespace-nowrap">
            <span className="text-on-surface-variant mr-1">
              {secondaryLabel}
            </span>
            {tableSecondary}
          </span>
        </span>
      </>
    ) : (
      <>
        <span className="flex h-5 min-w-0 items-baseline justify-between gap-3">
          <span
            className={cn(
              'truncate text-sm font-medium',
              label.mono && 'font-mono',
            )}
          >
            {label.text}
          </span>

          <span
            className={cn(
              'shrink-0 text-sm tabular-nums',
              first && 'text-primary font-medium',
            )}
          >
            {primary}
          </span>
        </span>

        <span className="text-on-surface-variant flex h-4 gap-3 text-xs tabular-nums">
          <span className="whitespace-nowrap">
            <span className="text-outline mr-1">↑</span>
            {traffic(upload)}
          </span>

          <span className="whitespace-nowrap">
            <span className="text-outline mr-1">↓</span>
            {traffic(download)}
          </span>

          <span className="truncate">{secondary}</span>
        </span>
      </>
    )

  return animated ? (
    <motion.button
      // Measured only when its place changes, not on every refresh.
      layout="position"
      layoutDependency={rank}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={transition}
      {...props}
    >
      {content}
    </motion.button>
  ) : (
    <button {...props}>{content}</button>
  )
}
