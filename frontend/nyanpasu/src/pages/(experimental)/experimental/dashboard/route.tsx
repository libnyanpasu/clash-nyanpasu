import { Button } from '@/components/ui/button'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(experimental)/experimental/dashboard')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <AppContentScrollArea>
      <div className="h-dvh">
        <p>Hello "/(experimental)/experimental/dashboard"!</p>

        <Button>Click me</Button>
      </div>
    </AppContentScrollArea>
  )
}
