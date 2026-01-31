import { createFileRoute, Navigate } from '@tanstack/react-router'

export const Route = createFileRoute('/(legacy)/')({
  component: RouteComponent,
})

function RouteComponent() {
  // const memorizedNavigate = useAtomValue(memorizedRoutePathAtom)

  return <Navigate to="/dashboard" />
}
