import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(main)/main/settings/clash-external-controll',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/settings/clash-external-controll"!</div>
}
