import { createFileRoute, Navigate } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/')({
  component: RouteComponent,
})

function RouteComponent() {
  return <Navigate to="/main/dashboard" />
}
