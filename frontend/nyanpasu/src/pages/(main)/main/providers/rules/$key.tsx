import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/providers/rules/$key')({
  component: RouteComponent,
})

function RouteComponent() {
  const { key } = Route.useParams()

  return <div>Hello "/(main)/main/providers/rules/${key}"!</div>
}
