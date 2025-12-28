import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(experimental)/experimental/profiles/inspect',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/profiles/inspect"!</div>
}
