import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/profiles/inspect"!</div>
}
