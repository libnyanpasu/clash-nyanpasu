import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { createFileRoute, Outlet } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/rules')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <AppContentScrollArea>
      <Outlet />
    </AppContentScrollArea>
  )
}
