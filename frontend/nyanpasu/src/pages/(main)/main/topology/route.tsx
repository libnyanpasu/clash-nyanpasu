import { keepReturn } from '@/components/router/cross-navigation'
import { ReturnButton } from '@/components/router/return-button'
import { useCrossNavigate } from '@/components/router/use-cross-navigate'
import {
  createFileRoute,
  redirect,
  useMatchRoute,
  useNavigate,
} from '@tanstack/react-router'
import { trafficSearchSchema } from './_modules/search'
import { TRAFFIC_PAGES } from './_modules/traffic-context'
import TrafficLayout from './_modules/traffic-layout'

export const Route = createFileRoute('/(main)/main/topology')({
  component: RouteComponent,
  validateSearch: trafficSearchSchema,
  beforeLoad: ({ location, search }) => {
    // Existing shared map/flow URLs open their new child route.
    if (
      location.pathname.replace(/\/$/, '') === '/main/topology' &&
      search.view &&
      search.view !== 'overview'
    ) {
      const page = TRAFFIC_PAGES.find((page) => page.view === search.view)!
      throw redirect({
        from: '/main/topology',
        to: page.to,
        search: { ...search, view: undefined },
        state: keepReturn,
        replace: true,
      })
    }
  },
})

function RouteComponent() {
  const search = Route.useSearch()
  const navigate = useNavigate()
  const matchRoute = useMatchRoute()
  const crossNavigate = useCrossNavigate()
  const page =
    TRAFFIC_PAGES.find(({ to }) =>
      matchRoute({ to, fuzzy: false, includeSearch: false }),
    ) ?? TRAFFIC_PAGES[0]
  const { scope, range, filters } = search

  return (
    <TrafficLayout
      view={page.view}
      search={search}
      onSearchChange={(update) =>
        navigate({
          to: page.to,
          search: (previous) => ({ ...previous, ...update, view: undefined }),
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
