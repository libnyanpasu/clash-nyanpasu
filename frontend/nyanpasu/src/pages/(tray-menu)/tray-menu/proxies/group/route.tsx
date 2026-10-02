import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { createFileRoute, Outlet } from '@tanstack/react-router'

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies/group')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <ScrollArea className="h-dvh w-dvw">
      <Outlet />
    </ScrollArea>
  )
}
