import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/system-service',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/settings/about"!</div>
}
