import { AnimatePresence } from 'motion/react'
import { useState, type ComponentType, type SVGProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card } from '@nyanpasu/ui/card'
import { m } from '@/paraglide/messages'
import { usageAmount } from '@/utils/traffic-usage'
import type { Ranking } from '@nyanpasu/rpc/types'
import { toggleFilter } from '../../_modules/traffic-filters'
import RankingModal from './ranking-modal'
import RankingRow from './ranking-row'
import { useTrafficPage } from './traffic-context'

function CountBadge({ count }: { count: number }) {
  return (
    <span className="bg-on-surface/8 text-on-surface-variant rounded-full px-1.5 text-[11px] leading-4 tabular-nums">
      {count.toLocaleString()}
    </span>
  )
}

export type RankingCardProps = {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  title: string
  ranking: Ranking
}

export default function RankingCard({
  icon: Icon,
  title,
  ranking,
}: RankingCardProps) {
  const [viewAll, setViewAll] = useState(false)
  const { search, labelOf, onSearchChange } = useTrafficPage()
  const { metric, filters } = search

  const { dimension } = ranking

  const selected = filters.find((filter) => filter.d === dimension)?.v

  const others = ranking.distinct - ranking.groups.length

  return (
    <Card className="flex flex-col gap-1.5 p-3" data-slot="traffic-ranking">
      <div className="flex min-w-0 items-center gap-1.5 px-1">
        <Icon className="text-on-surface-variant size-4 shrink-0" />

        {/* The title group takes the free space: the global unlayered
            `:where(button) { margin: 0 }` beats a `ml-auto` utility here. */}
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          <h2 className="truncate text-sm font-medium">{title}</h2>

          <CountBadge count={ranking.distinct} />
        </div>

        <Button
          className="h-7 shrink-0 px-2 text-xs whitespace-nowrap"
          onClick={() => setViewAll(true)}
        >
          {m.traffic_rank_view_all()}
        </Button>
      </div>

      <div className="flex min-w-0 flex-col gap-0.5">
        <AnimatePresence initial={false}>
          {ranking.groups.map((group, index) => (
            <RankingRow
              key={group.key}
              label={labelOf(dimension, group.key)}
              usage={group.usage}
              metric={metric}
              rank={index}
              active={group.key === selected}
              onSelect={() =>
                onSearchChange({
                  filters: toggleFilter(filters, dimension, group.key),
                })
              }
            />
          ))}
        </AnimatePresence>

        {others > 0 && (
          <p className="text-on-surface-variant truncate px-3 py-1.5 text-[11px] tabular-nums">
            {m.traffic_rank_other({ count: others.toLocaleString() })} ·{' '}
            {usageAmount(ranking.other, metric)}
          </p>
        )}
      </div>

      <RankingModal
        open={viewAll}
        onOpenChange={setViewAll}
        title={title}
        dimension={dimension}
      />
    </Card>
  )
}
