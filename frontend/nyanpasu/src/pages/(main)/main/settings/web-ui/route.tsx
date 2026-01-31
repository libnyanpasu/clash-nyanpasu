import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/settings/web-ui"!</div>
}
