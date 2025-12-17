import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/logs')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/logs"!</div>
}
