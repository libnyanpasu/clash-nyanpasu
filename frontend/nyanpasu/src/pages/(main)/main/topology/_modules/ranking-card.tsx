import { useState, type ComponentType, type SVGProps } from 'react'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import type { Dimension, Ranking, TrafficQuery } from '@nyanpasu/interface'
import RankingModal from './ranking-modal'
import RankingRow from './ranking-row'
import { setFilter, toggleFilter, type SearchFilter } from './search'
import type { UsageLabel } from './usage-label'

function CountBadge({ count }: { count: number }) {
  return (
    <span className="bg-on-surface/8 text-on-surface-variant rounded-full px-1.5 text-[11px] leading-4 tabular-nums">
      {count.toLocaleString()}
    </span>
  )
}

export default function RankingCard({
  icon: Icon,
  title,
  ranking,
  query,
  filters,
  labelOf,
  onFiltersChange,
}: {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  title: string
  ranking: Ranking
  query: TrafficQuery
  filters: SearchFilter[]
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onFiltersChange: (filters: SearchFilter[]) => void
}) {
  const [viewAll, setViewAll] = useState(false)

  const { dimension } = ranking

  const selected = filters.find((filter) => filter.d === dimension)?.v

  const others = ranking.distinct - ranking.groups.length

  return (
    <Card className="flex flex-col gap-2 p-4" data-slot="traffic-ranking">
      <div className="flex min-w-0 items-center gap-2 px-2">
        <Icon className="text-on-surface-variant size-5 shrink-0" />

        {/* The title group takes the free space: the global unlayered
            `:where(button) { margin: 0 }` beats a `ml-auto` utility here. */}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <h2 className="truncate text-base font-medium">{title}</h2>

          <CountBadge count={ranking.distinct} />
        </div>

        <Button
          className="h-8 shrink-0 px-3 whitespace-nowrap"
          onClick={() => setViewAll(true)}
        >
          {m.traffic_rank_view_all()}
        </Button>
      </div>

      <div className="flex flex-col gap-1">
        {ranking.groups.map((group, index) => (
          <RankingRow
            key={group.key}
            label={labelOf(dimension, group.key)}
            usage={group.usage}
            first={index === 0}
            active={group.key === selected}
            onSelect={() =>
              onFiltersChange(toggleFilter(filters, dimension, group.key))
            }
          />
        ))}

        {others > 0 && (
          <p className="text-on-surface-variant truncate px-4 py-2 text-xs tabular-nums">
            {m.traffic_rank_other({ count: others.toLocaleString() })} ·{' '}
            {parseTraffic(
              ranking.other.bytes.upload + ranking.other.bytes.download,
            ).join(' ')}
          </p>
        )}
      </div>

      <RankingModal
        open={viewAll}
        onOpenChange={setViewAll}
        title={title}
        dimension={dimension}
        // Every value of the dimension is a choice, so its own filter stays off.
        query={{
          ...query,
          filters: query.filters.filter((f) => f.dimension !== dimension),
        }}
        selected={selected}
        labelOf={labelOf}
        onSelect={(key) => {
          onFiltersChange(setFilter(filters, dimension, key))
          setViewAll(false)
        }}
      />
    </Card>
  )
}
