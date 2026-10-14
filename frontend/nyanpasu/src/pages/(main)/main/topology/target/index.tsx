import { createFileRoute } from '@tanstack/react-router'
import { m } from '@/paraglide/messages'
import Notice from '../_modules/notice'
import RankingView from '../_modules/ranking-view'
import { RANKINGS, useTrafficPage } from '../_modules/traffic-context'

export const Route = createFileRoute('/(main)/main/topology/target/')({
  component: TargetPage,
})

function TargetPage() {
  const { report, empty } = useTrafficPage()
  const configuration = RANKINGS.find((item) => item.dimension === 'target')!
  const ranking = report?.rankings.find((item) => item.dimension === 'target')

  if (empty) return <Notice>{m.traffic_empty()}</Notice>
  if (!ranking) return <Notice>{m.traffic_loading_more()}</Notice>

  return (
    <RankingView
      icon={configuration.icon}
      title={configuration.title()}
      ranking={ranking}
    />
  )
}
