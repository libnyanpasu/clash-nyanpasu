import { ReactElement, ReactNode } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import {
  SharedTooltip,
  SharedTooltipTrigger,
} from '@nyanpasu/ui/shared-tooltip'
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

// Rendered only while the tooltip is open, so the nodes of a grid do no
// history formatting when they re-render.
function DelayHistoryContent({ history }: { history: DelayHistorySample[] }) {
  const recent = history.slice(-HISTORY_LIMIT).reverse()
  const formatTime = (time: string) => {
    const date = new Date(time)
    return Number.isNaN(date.getTime())
      ? time
      : date.toLocaleString(getLocale(), { hour12: false })
  }

  return (
    <div className="space-y-2 py-1" data-slot="proxy-delay-history">
      <div className="font-medium">{m.proxies_delay_history_title()}</div>
      {recent.length === 0 ? (
        <EmptyState
          size="compact"
          tone="normal"
          message={m.proxies_delay_history_empty()}
        />
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

/**
 * The one tooltip that shows the delay history of the node under the pointer,
 * for every `DelayHistory` inside it. A tooltip per node mounted a popper
 * with each card of a group.
 */
export function DelayHistoryTooltip({ children }: { children: ReactNode }) {
  return (
    <SharedTooltip
      openDelay={300}
      className="bg-surface text-on-surface max-w-[calc(100vw-2rem)] rounded-xl px-3 py-2 text-xs shadow-lg"
    >
      {children}
    </SharedTooltip>
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
    <SharedTooltipTrigger content={<DelayHistoryContent history={history} />}>
      {children}
    </SharedTooltipTrigger>
  )
}
