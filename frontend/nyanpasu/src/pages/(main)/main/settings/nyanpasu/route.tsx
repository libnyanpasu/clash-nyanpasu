import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import BreakWhenModeChangeSwitch from './_modules/break-when-mode-change-switch'
import BreakWhenProfileChangeSwitch from './_modules/break-when-profile-change-switch'
import BreakWhenProxyChangeSwitch from './_modules/break-when-proxy-change-switch'
import EnableBuiltinEnhancedSwitch from './_modules/enable-builtin-enhanced-switch'
import HotkeyManager from './_modules/hotket-manager'
import LogFileConfig from './_modules/log-file-config'
import LogLevelSelector from './_modules/log-level-selector'
import NetworkStatisticWidgetSelector from './_modules/network-statistic-widget-selector'
import TrayIconConfig from './_modules/tray-icon-config'
import TrayMenuModeSelector from './_modules/tray-menu-mode'
import TrayProxiesSelector from './_modules/tray-proxies-selector'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
})

const LogSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_logs()}</SettingsLabel>

      <SettingsGroup>
        <LogLevelSelector />

        <LogFileConfig />
      </SettingsGroup>
    </div>
  )
}

const SystemWidgetSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>
        {m.settings_nyanpasu_network_statistic_widget_label()}
      </SettingsLabel>

      <SettingsGroup>
        <NetworkStatisticWidgetSelector />
      </SettingsGroup>
    </div>
  )
}

const EnhanceSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_enhance_label()}</SettingsLabel>

      <SettingsGroup>
        <BreakWhenProxyChangeSwitch />

        <BreakWhenProfileChangeSwitch />

        <BreakWhenModeChangeSwitch />

        <EnableBuiltinEnhancedSwitch />
      </SettingsGroup>
    </div>
  )
}

const TraySettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_tray()}</SettingsLabel>

      <SettingsGroup>
        <TrayMenuModeSelector />

        <TrayProxiesSelector />

        <TrayIconConfig />
      </SettingsGroup>
    </div>
  )
}

const KeyboardSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_keyboard_shortcuts()}</SettingsLabel>

      <SettingsGroup>
        <HotkeyManager />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_nyanpasu()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <LogSettings />

        <SystemWidgetSettings />

        <EnhanceSettings />

        <TraySettings />

        <KeyboardSettings />
      </div>
    </>
  )
}
