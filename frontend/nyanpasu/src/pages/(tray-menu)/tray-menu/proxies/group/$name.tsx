import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(tray-menu)/tray-menu/proxies/group/$name',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(tray-menu)/tray-menu/proxies/group/$name"!</div>
}
