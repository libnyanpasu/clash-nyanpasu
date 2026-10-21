import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import Notice from '../_modules/notice'
import RankingView from '../_modules/ranking-view'
import { RANKINGS, useTrafficPage } from '../_modules/traffic-context'

export const Route = createFileRoute('/(main)/main/topology/source/')({
  component: SourcePage,
})

function SourcePage() {
  const { report, empty } = useTrafficPage()
  const configuration = RANKINGS.find((item) => item.dimension === 'source')!
  const ranking = report?.rankings.find((item) => item.dimension === 'source')

  if (empty)
    return (
      <EmptyState className="min-h-40 flex-1" message={m.traffic_empty()} />
    )
  if (!ranking) return <Notice>{m.traffic_loading_more()}</Notice>

  return (
    <RankingView
      icon={configuration.icon}
      title={configuration.title()}
      ranking={ranking}
    />
  )
}
