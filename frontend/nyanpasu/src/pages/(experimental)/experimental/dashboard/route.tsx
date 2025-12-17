import { Button } from '@/components/ui/button'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/dashboard')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <div className="h-dvh">
      <p>Hello "/(experimental)/experimental/dashboard"!</p>

      <Button>Click me</Button>
    </div>
  )
}
