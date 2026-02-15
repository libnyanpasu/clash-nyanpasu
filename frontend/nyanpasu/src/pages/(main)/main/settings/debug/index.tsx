import { createFileRoute } from '@tanstack/react-router'
import AdvanceToolsSwitch from './_modules/advance-tools-switch'
import BlockTaskViewer from './_modules/block-task-viewer'
import { useDebugContext } from './_modules/debug-provider'
import WindowDebug from './_modules/window-debug'

export const Route = createFileRoute('/(main)/main/settings/debug/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { advanceTools } = useDebugContext()

  return (
    <div className="flex flex-col gap-4 p-4">
      <AdvanceToolsSwitch />

      {advanceTools && (
        <>
          <WindowDebug />

          <BlockTaskViewer />
        </>
      )}
    </div>
  )
}
