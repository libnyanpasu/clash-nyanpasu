import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/rules')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(experimental)/experimental/rules"!</div>
}
