import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import type { Usage } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import type { UsageLabel } from './usage-label'

const traffic = (bytes: number) => parseTraffic(bytes).join(' ')

export default function RankingRow({
  label,
  usage,
  first,
  active,
  onSelect,
}: {
  label: UsageLabel
  usage: Usage
  /** The heaviest group of its list. */
  first?: boolean
  /** Whether the group is already a filter. */
  active: boolean
  onSelect: () => void
}) {
  const { upload, download } = usage.bytes

  // The tooltip hangs on the button, so focusing the row with the keyboard
  // reveals the full label as well.
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
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
              {traffic(upload + download)}
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
              {m.topology_connection_count({ count: usage.connections })}
            </span>
          </span>
        </button>
      </TooltipTrigger>

      <TooltipContent className="max-w-96 rounded-2xl break-all">
        {label.title}
      </TooltipContent>
    </Tooltip>
  )
}
