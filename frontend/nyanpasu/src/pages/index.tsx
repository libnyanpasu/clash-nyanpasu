import { useAtomValue } from 'jotai'
import { memorizedRoutePathAtom } from '@/store'
import { getEnabledExperimentalRouter } from '@/utils/experimental'
import { createFileRoute, Navigate } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: RouteComponent,
})

function RouteComponent() {
  const memorizedNavigate = useAtomValue(memorizedRoutePathAtom)

  const isExperimentalRouterEnabled = getEnabledExperimentalRouter()

  if (isExperimentalRouterEnabled) {
    return <Navigate to="/experimental/dashboard" />
  }

  return <Navigate to={memorizedNavigate || '/dashboard'} />
}
