import { AnimatePresence } from 'motion/react'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card'
import AdvanceToolsSwitch from './_modules/advance-tools-switch'
import BlockTaskViewer from './_modules/block-task-viewer'
import ChangelogPreview from './_modules/changelog-preview'
import ConfigurationStatus from './_modules/configuration-status'
import CrashSimulation from './_modules/crash-simulation'
import { useDebugContext } from './_modules/debug-provider'
import ErrorReportingDebug from './_modules/error-reporting-debug'
import HttpServer from './_modules/http-server'
import KVStorage from './_modules/kv-storage'
import MockConnectionsSwitch from './_modules/mock-connections-switch'
import MockTrafficSwitch from './_modules/mock-traffic-switch'
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

              <ErrorReportingDebug />

              <CrashSimulation />

              <BlockTaskViewer />

              <KVStorage />

              <ChangelogPreview />

              <HttpServer />
              {import.meta.env.DEV && <MockConnectionsSwitch />}
              {import.meta.env.DEV && <MockTrafficSwitch />}
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
      <PageTitle backTo="/main/settings">{m.settings_label_debug()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <PathUtilsSettings />

        <ConfigurationStatus />

        <AdvanceToolsSettings />
      </div>
    </>
  )
}
