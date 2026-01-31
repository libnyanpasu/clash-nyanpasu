import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/system-behavior')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/settings/system-behavior"!</div>
}
