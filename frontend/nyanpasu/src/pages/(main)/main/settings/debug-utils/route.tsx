import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/debug-utils')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/settings/debug-utils"!</div>
}
