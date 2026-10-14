import { Button } from '@nyanpasu/ui/button'
import { keepReturn } from '@/components/router/cross-navigation'
import { m } from '@/paraglide/messages'
import { createFileRoute, Link } from '@tanstack/react-router'
import Notice from './_modules/notice'
import RankingCard from './_modules/ranking-card'
import StatCards from './_modules/stat-cards'
import {
  ANALYSIS_VIEWS,
  RANKINGS,
  useTrafficPage,
  withoutTrafficView,
} from './_modules/traffic-context'

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
          <div className="grid grid-cols-1 items-start gap-3 @2xl:grid-cols-2 @5xl:grid-cols-3">
            {RANKINGS.map(({ dimension, icon, title }) => {
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

            <div className="flex flex-col gap-3">
              {ANALYSIS_VIEWS.map(({ view, icon: Icon, title, to }) => (
                <Button
                  key={view}
                  asChild
                  variant="basic"
                  className="bg-surface flex h-auto min-h-16 items-center justify-start gap-3 rounded-3xl p-4 text-left"
                  data-slot="traffic-view-entry"
                >
                  <Link to={to} search={withoutTrafficView} state={keepReturn}>
                    <Icon className="text-on-surface-variant size-5 shrink-0" />
                    {title()}
                  </Link>
                </Button>
              ))}
            </div>
          </div>
        ))}
    </>
  )
}
