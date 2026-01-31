import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/providers')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/providers"!</div>
}
