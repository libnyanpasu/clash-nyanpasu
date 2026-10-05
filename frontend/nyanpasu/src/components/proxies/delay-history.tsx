import { ReactElement } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import type { DelayHistory as DelayHistorySample } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import DelayChip from './delay-chip'

const HISTORY_LIMIT = 10

export function DelayHistoryBar({
  history,
}: {
  history: DelayHistorySample[]
}) {
  return (
    <span
      className="flex h-1 w-12 shrink-0 gap-px overflow-hidden rounded-full"
      aria-hidden="true"
    >
      {history.slice(-HISTORY_LIMIT).map(({ delay }, index) => (
        <span
          key={index}
          className={cn(
            'h-full flex-1 bg-green-500',
            delay > 100 && 'bg-yellow-500',
            delay > 300 && 'bg-orange-500',
            (delay > 500 || delay <= 0) && 'bg-red-500',
          )}
        />
      ))}
    </span>
  )
}

// Rendered only while the tooltip is open, so closed tooltips on a node grid
// do no history formatting when their node re-renders.
function DelayHistoryContent({ history }: { history: DelayHistorySample[] }) {
  const recent = history.slice(-HISTORY_LIMIT).reverse()
  const formatTime = (time: string) => {
    const date = new Date(time)
    return Number.isNaN(date.getTime())
      ? time
      : date.toLocaleString(getLocale(), { hour12: false })
  }

  return (
    <div className="space-y-2 py-1">
      <div className="font-medium">{m.proxies_delay_history_title()}</div>
      {recent.length === 0 ? (
        <div>{m.proxies_delay_history_empty()}</div>
      ) : (
        <>
          <div className="text-on-surface-variant">
            {m.proxies_delay_history_recent({ count: recent.length })}
          </div>
          <ol className="space-y-1">
            {recent.map(({ time, delay }, index) => (
              <li
                key={`${time}-${index}`}
                className="flex items-center justify-between gap-4 tabular-nums"
              >
                <time dateTime={time}>{formatTime(time)}</time>
                {delay > 0 ? (
                  <DelayChip delay={delay} />
                ) : (
                  <span className="text-red-500">
                    {m.proxies_delay_history_failed()}
                  </span>
                )}
              </li>
            ))}
          </ol>
        </>
      )}
    </div>
  )
}

export default function DelayHistory({
  history = [],
  children,
}: {
  history?: DelayHistorySample[]
  children: ReactElement
}) {
  return (
    <Tooltip delayDuration={300}>
      <TooltipTrigger asChild>{children}</TooltipTrigger>

      <TooltipContent
        sideOffset={8}
        className="bg-surface text-on-surface z-50 max-w-[calc(100vw-2rem)] rounded-xl px-3 py-2 text-xs shadow-lg"
        data-slot="proxy-delay-history"
      >
        <DelayHistoryContent history={history} />
      </TooltipContent>
    </Tooltip>
  )
}
