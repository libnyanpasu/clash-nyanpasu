import { Tooltip } from 'radix-ui'
import { ReactElement } from 'react'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { ProxyItemHistory } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import DelayChip from './delay-chip'

const HISTORY_LIMIT = 10

export function DelayHistoryBar({ history }: { history: ProxyItemHistory[] }) {
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

export default function DelayHistory({
  history = [],
  children,
}: {
  history?: ProxyItemHistory[]
  children: ReactElement
}) {
  const recent = history.slice(-HISTORY_LIMIT).reverse()
  const formatTime = (time: string) => {
    const date = new Date(time)
    return Number.isNaN(date.getTime())
      ? time
      : date.toLocaleString(getLocale(), { hour12: false })
  }

  return (
    <Tooltip.Provider delayDuration={300}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content
            sideOffset={8}
            className="bg-surface text-on-surface z-50 max-w-[calc(100vw-2rem)] rounded-xl px-3 py-2 text-xs shadow-lg"
            data-slot="proxy-delay-history"
          >
            <div className="space-y-2 py-1">
              <div className="font-medium">
                {m.proxies_delay_history_title()}
              </div>
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
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  )
}
