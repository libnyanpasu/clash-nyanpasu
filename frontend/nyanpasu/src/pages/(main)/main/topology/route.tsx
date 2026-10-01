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

  return (
    <TrafficPage
      search={search}
      onSearchChange={(update) =>
        navigate({ search: (previous) => ({ ...previous, ...update }) })
      }
    />
  )
}
