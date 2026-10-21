import { AnimatePresence } from 'motion/react'
import { useDeferredValue, type ComponentType, type SVGProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card } from '@nyanpasu/ui/card'
import { keepReturn } from '@/components/router/cross-navigation'
import { m } from '@/paraglide/messages'
import { usageAmount } from '@/utils/traffic-usage'
import type { Ranking, Usage } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { toggleFilter } from '../../_modules/traffic-filters'
import RankingRow, { CARD_ROW_HEIGHT, RankingRowTooltip } from './ranking-row'
import {
  useTrafficPage,
  withoutTrafficView,
  type RankingDimension,
} from './traffic-context'

function CountBadge({ count }: { count: number }) {
  return (
    <span className="bg-on-surface/8 text-on-surface-variant rounded-full px-1.5 text-[11px] leading-4 tabular-nums">
      {count.toLocaleString()}
    </span>
  )
}

const without = (usage: Usage, part: Usage): Usage => ({
  bytes: {
    upload: Math.max(0, usage.bytes.upload - part.bytes.upload),
    download: Math.max(0, usage.bytes.download - part.bytes.download),
  },
  connections: Math.max(0, usage.connections - part.connections),
})

/**
 * The filtered value of a ranking. A ranking ignores the filter on its own
 * dimension, so the value's group is the report's total. Outside the top
 * groups, it is pinned below them and leaves the "other" line.
 */
function useRankingSelection(ranking: Ranking) {
  const { search, report } = useTrafficPage()

  const selected = search.filters.find(
    (filter) => filter.d === ranking.dimension,
  )?.v
  const pinned =
    selected !== undefined &&
    report &&
    !ranking.groups.some((group) => group.key === selected)
      ? { key: selected, usage: report.total }
      : undefined

  return {
    selected,
    pinned,
    others: Math.max(
      0,
      ranking.distinct - ranking.groups.length - (pinned ? 1 : 0),
    ),
    other: pinned ? without(ranking.other, pinned.usage) : ranking.other,
  }
}

export default function RankingCard({
  dimension,
  icon: Icon,
  title,
  ranking,
}: {
  dimension: RankingDimension
  icon: ComponentType<SVGProps<SVGSVGElement>>
  title: string
  ranking: Ranking
}) {
  const { search, labelOf, onSearchChange } = useTrafficPage()
  const { metric, filters } = search

  const { selected, pinned, others, other } = useRankingSelection(ranking)

  // The card's frame shows with the page and its rows render in the pass
  // after, so opening a page is not one long task. Until then their place is
  // kept at the height they take, so nothing moves when they fill in.
  const rowsShown = useDeferredValue(true, false)
  const rows = ranking.groups.length + (pinned ? 1 : 0)

  const toggle = (key: string) =>
    onSearchChange({ filters: toggleFilter(filters, dimension, key) })

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

        {/* A link centers its text itself; a button gets it from the native element. */}
        <Button
          asChild
          className="inline-flex h-7 shrink-0 items-center px-2 text-xs whitespace-nowrap"
        >
          <Link
            from="/main/topology"
            to="/main/topology/$dimension"
            params={{ dimension }}
            search={withoutTrafficView}
            state={keepReturn}
          >
            {m.traffic_rank_view_all()}
          </Link>
        </Button>
      </div>

      <div className="flex min-w-0 flex-col gap-0.5">
        {!rowsShown && rows > 0 && (
          <div
            aria-hidden
            data-slot="traffic-ranking-rows-placeholder"
            style={{
              height: `calc(${rows} * ${CARD_ROW_HEIGHT} + ${rows - 1} * 0.125rem)`,
            }}
          />
        )}

        {rowsShown && (
          <RankingRowTooltip>
            {/* A leaving row goes at once and the others measure their place
              when their rank changes, so the rows need not render again
              whenever the presence of one changes. */}
            <AnimatePresence initial={false} presenceAffectsLayout={false}>
              {ranking.groups.map((group, index) => (
                <RankingRow
                  key={group.key}
                  label={labelOf(dimension, group.key)}
                  usage={group.usage}
                  metric={metric}
                  rank={index}
                  active={group.key === selected}
                  onSelect={() => toggle(group.key)}
                />
              ))}

              {pinned && (
                <RankingRow
                  key={pinned.key}
                  label={labelOf(dimension, pinned.key)}
                  usage={pinned.usage}
                  metric={metric}
                  rank={null}
                  active
                  onSelect={() => toggle(pinned.key)}
                />
              )}
            </AnimatePresence>
          </RankingRowTooltip>
        )}

        {others > 0 && (
          <p className="text-on-surface-variant truncate px-3 py-1.5 text-[11px] tabular-nums">
            {m.traffic_rank_other({ count: others.toLocaleString() })} ·{' '}
            {usageAmount(other, metric)}
          </p>
        )}
      </div>
    </Card>
  )
}
