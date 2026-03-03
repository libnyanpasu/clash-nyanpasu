import { AnimatePresence } from 'framer-motion'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import AdvanceToolsSwitch from './_modules/advance-tools-switch'
import BlockTaskViewer from './_modules/block-task-viewer'
import { useDebugContext } from './_modules/debug-provider'
import PathUtilsCard from './_modules/path-utils-card'
import WindowDebug from './_modules/window-debug'

export const Route = createFileRoute('/(main)/main/settings/debug/')({
  component: RouteComponent,
})

const PathUtilsSettings = () => {
  return (
    <div data-slot="debug-settings-container">
      <SettingsLabel>{m.settings_label_debug()}</SettingsLabel>

      <PathUtilsCard />
    </div>
  )
}

const AdvanceToolsSettings = () => {
  const { advanceTools } = useDebugContext()

  return (
    <div data-slot="debug-settings-container">
      <SettingsLabel>Advance Tools</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AdvanceToolsSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <AnimatePresence initial={false}>
          {advanceTools && (
            <>
              <WindowDebug />

              <BlockTaskViewer />
            </>
          )}
        </AnimatePresence>
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_debug()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <PathUtilsSettings />

        <AdvanceToolsSettings />
      </div>
    </>
  )
}
