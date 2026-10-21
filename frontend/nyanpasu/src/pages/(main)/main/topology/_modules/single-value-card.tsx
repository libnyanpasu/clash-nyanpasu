import type { ComponentType, SVGProps } from 'react'
import { Card } from '@nyanpasu/ui/card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { dimensionName, usageAmount } from '@/utils/traffic-usage'
import type { Ranking } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { toggleFilter } from '../../_modules/traffic-filters'
import { useTrafficPage } from './traffic-context'

/**
 * The rankings whose traffic all shares one value, a line each: a ranking card
 * for them would show one row under its header. On a single machine, the
 * source and the inbound usually look like this.
 */
export default function SingleValueCard({
  items,
}: {
  items: {
    icon: ComponentType<SVGProps<SVGSVGElement>>
    ranking: Ranking
  }[]
}) {
  const { search, labelOf, onSearchChange } = useTrafficPage()
  const { metric, filters } = search

  return (
    <Card
      className="flex flex-col gap-1.5 p-3"
      data-slot="traffic-single-values"
    >
      <h2 className="truncate px-1 text-sm font-medium">
        {m.traffic_rank_single()}
      </h2>

      <div
        role="group"
        aria-label={m.traffic_rank_single()}
        className="flex min-w-0 flex-col gap-0.5"
      >
        {items.map(({ icon: Icon, ranking: { dimension, groups } }) => {
          const [group] = groups
          const label = labelOf(dimension, group.key)
          const active = filters.some(
            (filter) => filter.d === dimension && filter.v === group.key,
          )

          return (
            <Tooltip key={dimension}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  aria-pressed={active}
                  className={cn(
                    'flex w-full min-w-0 cursor-pointer items-center gap-2 rounded-xl px-3 py-2 text-left text-sm outline-none',
                    'focus-visible:ring-primary focus-visible:ring-2',
                    'hover:bg-on-surface/8 transition-colors',
                    active && 'bg-secondary-container',
                  )}
                  onClick={() =>
                    onSearchChange({
                      filters: toggleFilter(filters, dimension, group.key),
                    })
                  }
                >
                  <Icon className="text-on-surface-variant size-4 shrink-0" />

                  <span className="text-on-surface-variant shrink-0">
                    {dimensionName(dimension)}
                  </span>

                  <span
                    className={cn(
                      'min-w-0 flex-1 truncate font-medium',
                      label.mono && 'font-mono',
                    )}
                  >
                    {label.text}
                  </span>

                  <span className="shrink-0 tabular-nums">
                    {usageAmount(group.usage, metric)}
                  </span>
                </button>
              </TooltipTrigger>

              <TooltipContent className="max-w-96 rounded-2xl break-all">
                {label.title}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>
    </Card>
  )
}
