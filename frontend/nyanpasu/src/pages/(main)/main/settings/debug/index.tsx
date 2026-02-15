import { Separator } from '@/components/ui/separator'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import AdvanceToolsSwitch from './_modules/advance-tools-switch'
import BlockTaskViewer from './_modules/block-task-viewer'
import { useDebugContext } from './_modules/debug-provider'
import PathUtilsCard from './_modules/path-utils-card'
import WindowDebug from './_modules/window-debug'

export const Route = createFileRoute('/(main)/main/settings/debug/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { advanceTools } = useDebugContext()

  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_label_debug()}</SettingsTitle>

      <PathUtilsCard />

      <Separator className="my-4" />

      <AdvanceToolsSwitch />

      {advanceTools && (
        <>
          <WindowDebug />

          <BlockTaskViewer />
        </>
      )}
    </>
  )
}
