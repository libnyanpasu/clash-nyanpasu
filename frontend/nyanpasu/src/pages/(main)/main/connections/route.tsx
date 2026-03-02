import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { createFileRoute, Outlet } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/connections')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <AppContentScrollArea scrollbars="both" type="auto">
      <Outlet />
    </AppContentScrollArea>
  )
}
