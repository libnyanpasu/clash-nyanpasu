import { Card } from '@nyanpasu/ui/card'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import Notice from '../_modules/notice'
import { useTrafficPage } from '../_modules/traffic-context'
import GeographyView from './_modules/geography-view'

export const Route = createFileRoute('/(main)/main/topology/map/')({
  component: TrafficMapPage,
})

function TrafficMapPage() {
  const { report } = useTrafficPage()

  if (!report) return <Notice>{m.traffic_loading_more()}</Notice>

  return (
    <Card className="p-3 md:p-4">
      <GeographyView topology={report.topology} />
    </Card>
  )
}
