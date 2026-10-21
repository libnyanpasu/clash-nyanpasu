import { useCallback, useMemo } from 'react'
import { keepReturn } from '@/components/router/cross-navigation'
import { ReturnButton } from '@/components/router/return-button'
import { useCrossNavigate } from '@/components/router/use-cross-navigate'
import {
  createFileRoute,
  redirect,
  useMatchRoute,
  useNavigate,
} from '@tanstack/react-router'
import { trafficSearchSchema, type TrafficSearch } from './_modules/search'
import { RANKINGS, TRAFFIC_PAGES } from './_modules/traffic-context'
import TrafficLayout from './_modules/traffic-layout'

export const Route = createFileRoute('/(main)/main/topology')({
  component: RouteComponent,
  validateSearch: trafficSearchSchema,
  beforeLoad: ({ location, search }) => {
    // Existing shared URLs choose a child route through their view.
    const { view } = search
    if (
      location.pathname.replace(/\/$/, '') !== '/main/topology' ||
      !view ||
      view === 'overview'
    ) {
      return
    }
    const ranking = RANKINGS.find(({ dimension }) => dimension === view)
    throw redirect({
      from: '/main/topology',
      ...(ranking
        ? {
            to: '/main/topology/$dimension',
            params: { dimension: ranking.dimension },
          }
        : {
            to: view === 'flow' ? '/main/topology/flow' : '/main/topology/map',
          }),
      search: { ...search, view: undefined },
      state: keepReturn,
      replace: true,
    })
  },
})

function RouteComponent() {
  const search = Route.useSearch()
  const navigate = useNavigate()
  const matchRoute = useMatchRoute()
  const crossNavigate = useCrossNavigate()
  const page =
    TRAFFIC_PAGES.find((item) =>
      matchRoute({
        to: item.to,
        params: 'params' in item ? item.params : undefined,
        fuzzy: false,
        includeSearch: false,
      }),
    ) ?? TRAFFIC_PAGES[0]
  const { scope, range, filters } = search

  // Stable, so switching between the pages leaves the page context alone.
  const handleSearchChange = useCallback(
    (update: Partial<TrafficSearch>) =>
      navigate({
        // The current child page, whichever it is.
        to: '.',
        search: (previous: TrafficSearch) => ({
          ...previous,
          ...update,
          view: undefined,
        }),
        state: keepReturn,
      }),
    [navigate],
  )
  const handleViewConnections = useCallback(
    () =>
      crossNavigate({
        from: 'traffic',
        to: { to: '/main/connections', search: { scope, range, filters } },
      }),
    [crossNavigate, scope, range, filters],
  )
  const toolbarStart = useMemo(() => <ReturnButton />, [])

  return (
    <TrafficLayout
      view={page.view}
      search={search}
      onSearchChange={handleSearchChange}
      onViewConnections={handleViewConnections}
      toolbarStart={toolbarStart}
    />
  )
}
