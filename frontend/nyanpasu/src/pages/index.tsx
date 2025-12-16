import { useAtomValue } from 'jotai'
import { memorizedRoutePathAtom } from '@/store'
import { createFileRoute, Navigate } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: RouteComponent,
})

function RouteComponent() {
  const memorizedNavigate = useAtomValue(memorizedRoutePathAtom)

  return <Navigate to={memorizedNavigate || '/dashboard'} />
}
