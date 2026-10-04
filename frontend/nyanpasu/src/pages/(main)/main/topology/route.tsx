import { keepReturn } from '@/components/router/cross-navigation'
import { ReturnButton } from '@/components/router/return-button'
import { useCrossNavigate } from '@/components/router/use-cross-navigate'
import { createFileRoute } from '@tanstack/react-router'
import { trafficSearchSchema } from './_modules/search'
import TrafficPage from './_modules/traffic-page'

export const Route = createFileRoute('/(main)/main/topology')({
  component: RouteComponent,
  validateSearch: trafficSearchSchema,
})

function RouteComponent() {
  const search = Route.useSearch()
  const navigate = Route.useNavigate()

  const crossNavigate = useCrossNavigate()

  const { scope, range, filters } = search

  return (
    <TrafficPage
      search={search}
      onSearchChange={(update) =>
        navigate({
          search: (previous) => ({ ...previous, ...update }),
          state: keepReturn,
        })
      }
      onViewConnections={() =>
        crossNavigate({
          from: 'traffic',
          to: { to: '/main/connections', search: { scope, range, filters } },
        })
      }
      toolbarStart={<ReturnButton />}
    />
  )
}
