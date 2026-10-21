import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card'
import BreakWhenModeChangeSwitch from './_modules/break-when-mode-change-switch'
import BreakWhenProfileChangeSwitch from './_modules/break-when-profile-change-switch'
import BreakWhenProxyChangeSelector from './_modules/break-when-proxy-change-selector'
import CoreLogStorageConfig from './_modules/core-log-storage-config'
import LatencyTestConfig from './_modules/latency-test-config'
import LogLevelSelector from './_modules/log-level-selector'
import ManagedGuardFieldSelector from './_modules/managed-guard-field-selector'

export const Route = createFileRoute('/(main)/main/settings/clash/core/')({
  component: RouteComponent,
})

const CoreBehaviorSettings = () => {
  return (
    <div data-slot="core-behavior-settings-container">
      <SettingsLabel>{m.settings_clash_core_behavior_label()}</SettingsLabel>

      <SettingsGroup>
        <ManagedGuardFieldSelector field="unified-delay" />

        <ManagedGuardFieldSelector field="tcp-concurrent" />
      </SettingsGroup>
    </div>
  )
}

const LatencyTestSettings = () => {
  return (
    <div data-slot="latency-test-settings-container">
      <SettingsLabel>{m.settings_clash_latency_test_label()}</SettingsLabel>

      <SettingsGroup>
        <LatencyTestConfig />
      </SettingsGroup>
    </div>
  )
}

const ConnectionSettings = () => {
  return (
    <div data-slot="connection-settings-container">
      <SettingsLabel>{m.settings_clash_core_connections_label()}</SettingsLabel>

      <SettingsGroup>
        <BreakWhenProxyChangeSelector />

        <BreakWhenProfileChangeSwitch />

        <BreakWhenModeChangeSwitch />
      </SettingsGroup>
    </div>
  )
}

const CoreLogSettings = () => {
  return (
    <div data-slot="core-log-settings-container">
      <SettingsLabel>{m.settings_logs_core_label()}</SettingsLabel>

      <SettingsGroup>
        <LogLevelSelector />

        <CoreLogStorageConfig />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_core_settings()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <CoreBehaviorSettings />

        <LatencyTestSettings />

        <ConnectionSettings />

        <CoreLogSettings />
      </div>
    </>
  )
}
