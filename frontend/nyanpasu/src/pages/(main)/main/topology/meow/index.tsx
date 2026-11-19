import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { useTrafficPage } from '../_modules/traffic-context'
import MeowTraffic from './_modules/meow-traffic'

export const Route = createFileRoute('/(main)/main/topology/meow/')({
  component: MeowTrafficPage,
})

function MeowTrafficPage() {
  const { meowCore } = useTrafficPage()
  return meowCore ? (
    <MeowTraffic key={meowCore} core={meowCore} />
  ) : (
    <EmptyState message={m.meow_traffic_unavailable_core()} />
  )
}
