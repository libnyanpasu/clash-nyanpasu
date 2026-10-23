import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../../../_modules/settings-card'
import AppLogLevelSelector from './_modules/app-log-level-selector'
import CoreLogLevelSelector from './_modules/core-log-level-selector'
import CoreLogStorageConfig from './_modules/core-log-storage-config'
import LocalIpProbeSwitch from './_modules/local-ip-probe-switch'
import LogFileConfig from './_modules/log-file-config'
import TrafficRetentionSelector from './_modules/traffic-retention-selector'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu/logs/')({
  component: RouteComponent,
})

const AppLogSettings = () => {
  return (
    <div data-slot="app-log-settings-container">
      <SettingsLabel>{m.settings_logs_app_label()}</SettingsLabel>

      <SettingsGroup>
        <AppLogLevelSelector />

        <LogFileConfig />
      </SettingsGroup>
    </div>
  )
}

const CoreLogSettings = () => {
  return (
    <div data-slot="core-log-settings-container">
      <SettingsLabel>{m.settings_logs_core_label()}</SettingsLabel>

      <SettingsGroup>
        <CoreLogLevelSelector />

        <CoreLogStorageConfig />
      </SettingsGroup>
    </div>
  )
}

const TrafficSettings = () => {
  return (
    <div data-slot="traffic-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_traffic_label()}</SettingsLabel>

      <SettingsGroup>
        <TrafficRetentionSelector />

        <LocalIpProbeSwitch />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_logs()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <AppLogSettings />

        <CoreLogSettings />

        <TrafficSettings />
      </div>
    </>
  )
}
