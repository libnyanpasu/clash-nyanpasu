import { createFileRoute, Outlet } from '@tanstack/react-router'
import { DashboardProvider } from './_modules/provider'

export const Route = createFileRoute('/(main)/main/dashboard')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <DashboardProvider>
      <Outlet />
    </DashboardProvider>
  )
}
