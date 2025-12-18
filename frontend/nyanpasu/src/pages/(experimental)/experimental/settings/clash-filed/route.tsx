import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/clash-filed',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/settings/clash-filed"!</div>
}
