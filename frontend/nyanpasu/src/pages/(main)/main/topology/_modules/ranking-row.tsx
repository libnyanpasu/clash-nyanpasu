import { motion } from 'motion/react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import parseTraffic from '@/utils/parse-traffic'
import { usageAmount, type UsageLabel } from '@/utils/traffic-usage'
import type { Metric, Usage } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { usePageTransition } from './transition'

const traffic = (bytes: number) => parseTraffic(bytes).join(' ')

/**
 * A group of a ranking. Inside `AnimatePresence`, a new row fades in and a
 * row whose place changes slides there. A leaving row goes at once: when a
 * filter replaces the whole list, animating every row out stalls the page.
 */
export default function RankingRow({
  label,
  usage,
  metric,
  rank,
  active,
  onSelect,
}: {
  label: UsageLabel
  usage: Usage
  /** The amount the row leads with; the other one closes its second line. */
  metric: Metric
  /** Its place in the list, 0 for the heaviest group. */
  rank: number
  /** Whether the group is already a filter. */
  active: boolean
  onSelect: () => void
}) {
  const { transition } = usePageTransition()

  const { upload, download } = usage.bytes

  const first = rank === 0

  // The tooltip hangs on the button, so focusing the row with the keyboard
  // reveals the full label as well.
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <motion.button
          // Measured only when its place changes, not on every refresh.
          layout="position"
          layoutDependency={rank}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={transition}
          type="button"
          aria-pressed={active}
          className={cn(
            'flex w-full min-w-0 cursor-pointer flex-col gap-0.5 rounded-2xl px-4 py-3 text-left outline-none',
            'focus-visible:ring-primary focus-visible:ring-2',
            'hover:bg-on-surface/8 transition-colors',
            first && 'bg-secondary-container/60',
            active && 'bg-secondary-container',
          )}
          onClick={onSelect}
        >
          <span className="flex min-w-0 items-baseline justify-between gap-3">
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
              {usageAmount(usage, metric)}
            </span>
          </span>

          <span className="text-on-surface-variant flex gap-3 text-xs tabular-nums">
            <span className="whitespace-nowrap">
              <span className="text-outline mr-1">↑</span>
              {traffic(upload)}
            </span>

            <span className="whitespace-nowrap">
              <span className="text-outline mr-1">↓</span>
              {traffic(download)}
            </span>

            <span className="truncate">
              {usageAmount(usage, metric === 'bytes' ? 'connections' : 'bytes')}
            </span>
          </span>
        </motion.button>
      </TooltipTrigger>

      <TooltipContent className="max-w-96 rounded-2xl break-all">
        {label.title}
      </TooltipContent>
    </Tooltip>
  )
}
