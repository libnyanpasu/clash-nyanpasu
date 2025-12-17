import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/profiles')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/profiles"!</div>
}
