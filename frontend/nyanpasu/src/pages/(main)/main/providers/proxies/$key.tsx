import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/providers/proxies/$key')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/(main)/main/providers/proxies/"!</div>
}
