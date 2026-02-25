import { cn } from '@nyanpasu/ui'
import { createFileRoute, Outlet } from '@tanstack/react-router'
import '@/services/monaco'

export const Route = createFileRoute('/(editor)/editor')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <div
      className={cn('flex h-dvh flex-col', 'bg-background/30')}
      data-slot="editor-container"
      onContextMenu={(e) => {
        e.preventDefault()
      }}
    >
      <Outlet />
    </div>
  )
}
