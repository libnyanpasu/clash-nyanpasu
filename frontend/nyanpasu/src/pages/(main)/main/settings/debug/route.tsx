import { createFileRoute } from '@tanstack/react-router'
import BlockTaskViewer from './_modules/block-task-viewer'
import WindowDebug from './_modules/window-debug'

export const Route = createFileRoute('/(main)/main/settings/debug')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <div className="flex flex-col gap-4 p-4">
      <WindowDebug />

      <BlockTaskViewer />
    </div>
  )
}
