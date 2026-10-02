import { DashboardProvider } from '@/components/widgets/provider'
import { createFileRoute, Outlet } from '@tanstack/react-router'

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
