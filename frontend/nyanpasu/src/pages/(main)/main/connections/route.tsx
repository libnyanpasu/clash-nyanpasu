import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/connections')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/connections"!</div>
}
