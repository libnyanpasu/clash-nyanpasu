import { z } from 'zod'
import { cn } from '@nyanpasu/ui'
import { createFileRoute, Outlet } from '@tanstack/react-router'
import Header from './_modules/header'
import '@/services/monaco'

export const Route = createFileRoute('/(editor)/editor')({
  component: RouteComponent,
  validateSearch: z.object({
    uid: z.string(),
    readonly: z.boolean().optional().default(false),
  }),
})

function RouteComponent() {
  const { uid } = Route.useSearch()

  return (
    <div
      className={cn('flex h-dvh flex-col', 'bg-background/30')}
      data-slot="editor-container"
      data-editor-uid={uid}
      onContextMenu={(e) => {
        e.preventDefault()
      }}
    >
      <Header />

      <Outlet />
    </div>
  )
}
