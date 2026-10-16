import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import Notice from './_modules/notice'
import RankingCard from './_modules/ranking-card'
import StatCards from './_modules/stat-cards'
import { RANKINGS, useTrafficPage } from './_modules/traffic-context'

export const Route = createFileRoute('/(main)/main/topology/')({
  component: TrafficOverviewPage,
})

function TrafficOverviewPage() {
  const { report, empty } = useTrafficPage()

  return (
    <>
      <StatCards report={report} />

      {report &&
        (empty ? (
          <Notice>{m.traffic_empty()}</Notice>
        ) : (
          <div
            className="-mb-3 columns-1 gap-x-3 @2xl:columns-2 @5xl:columns-3"
            data-slot="traffic-overview-masonry"
          >
            {RANKINGS.map(({ dimension, icon, title }) => {
              const ranking = report.rankings.find(
                (item) => item.dimension === dimension,
              )

              return (
                ranking && (
                  <div
                    key={dimension}
                    className="break-inside-avoid pb-3"
                    data-slot="traffic-overview-masonry-item"
                  >
                    <RankingCard
                      icon={icon}
                      title={title()}
                      ranking={ranking}
                    />
                  </div>
                )
              )
            })}
          </div>
        ))}
    </>
  )
}
