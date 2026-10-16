import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import RankingCard from './_modules/ranking-card'
import SingleValueCard from './_modules/single-value-card'
import StatCards from './_modules/stat-cards'
import { RANKINGS, useTrafficPage } from './_modules/traffic-context'

export const Route = createFileRoute('/(main)/main/topology/')({
  component: TrafficOverviewPage,
})

function TrafficOverviewPage() {
  const { report, empty } = useTrafficPage()

  const rankings = RANKINGS.flatMap(({ dimension, icon, title }) => {
    const ranking = report?.rankings.find(
      (item) => item.dimension === dimension,
    )

    return ranking ? [{ dimension, icon, title, ranking }] : []
  })
  const single = rankings.filter(({ ranking }) => ranking.distinct === 1)

  return (
    <>
      <StatCards report={report} />

      {report &&
        (empty ? (
          <EmptyState className="min-h-40 flex-1" message={m.traffic_empty()} />
        ) : (
          <div
            className="-mb-3 columns-1 gap-x-3 @2xl:columns-2 @5xl:columns-3"
            data-slot="traffic-overview-masonry"
          >
            {rankings.map(
              ({ dimension, icon, title, ranking }) =>
                ranking.distinct !== 1 && (
                  <div
                    key={dimension}
                    className="break-inside-avoid pb-3"
                    data-slot="traffic-overview-masonry-item"
                  >
                    <RankingCard
                      dimension={dimension}
                      icon={icon}
                      title={title()}
                      ranking={ranking}
                    />
                  </div>
                ),
            )}

            {single.length > 0 && (
              <div
                className="break-inside-avoid pb-3"
                data-slot="traffic-overview-masonry-item"
              >
                <SingleValueCard items={single} />
              </div>
            )}
          </div>
        ))}
    </>
  )
}
