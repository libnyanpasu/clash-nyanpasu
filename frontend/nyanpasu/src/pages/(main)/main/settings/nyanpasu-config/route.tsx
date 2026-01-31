import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu-config')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/settings/nyanpasu-config"!</div>
}
