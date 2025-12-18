import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/clash-settings',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <div>Hello "/(experimental)/experimental/settings/clash-settings"!</div>
  )
}
