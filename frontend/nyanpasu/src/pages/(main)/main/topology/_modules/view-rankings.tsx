import { m } from '@/paraglide/messages'
import RankingCard from './ranking-card'
import { RANKINGS, useTrafficPage } from './traffic-context'

/**
 * The most asked-for rankings under a drawing. A node or a region picked in
 * the drawing filters them in place, so its result reads on the same page.
 */
export default function ViewRankings() {
  const { report, empty } = useTrafficPage()

  if (!report || empty) return null

  return (
    <section
      className="flex flex-col gap-2"
      aria-label={m.traffic_view_rankings()}
      data-slot="traffic-view-rankings"
    >
      <h2 className="text-on-surface-variant px-1 text-sm font-medium">
        {m.traffic_view_rankings()}
      </h2>

      <div className="grid grid-cols-1 items-start gap-3 @2xl:grid-cols-2 @5xl:grid-cols-3">
        {/* The rankings come by how often they are needed. */}
        {RANKINGS.slice(0, 3).map(({ dimension, icon, title }) => {
          const ranking = report.rankings.find(
            (item) => item.dimension === dimension,
          )

          return (
            ranking && (
              <RankingCard
                key={dimension}
                icon={icon}
                title={title()}
                ranking={ranking}
              />
            )
          )
        })}
      </div>
    </section>
  )
}
