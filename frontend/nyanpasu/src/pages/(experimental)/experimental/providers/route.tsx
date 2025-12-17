import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/providers')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/providers"!</div>
}
