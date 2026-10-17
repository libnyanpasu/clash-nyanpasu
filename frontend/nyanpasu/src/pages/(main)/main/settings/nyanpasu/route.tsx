import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { isMacOS } from '@nyanpasu/platform'
import { useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card'
import BreakWhenModeChangeSwitch from './_modules/break-when-mode-change-switch'
import BreakWhenProfileChangeSwitch from './_modules/break-when-profile-change-switch'
import BreakWhenProxyChangeSelector from './_modules/break-when-proxy-change-selector'
import EnableBuiltinEnhancedSwitch from './_modules/enable-builtin-enhanced-switch'
import HotkeyManager from './_modules/hotket-manager'
import LocalIpProbeSwitch from './_modules/local-ip-probe-switch'
import LogFileConfig from './_modules/log-file-config'
import LogLevelSelector from './_modules/log-level-selector'
import NetworkStatisticWidgetIconConfig from './_modules/network-statistic-widget-icon-config'
import NetworkStatisticWidgetLogoFollowStateSwitch from './_modules/network-statistic-widget-logo-follow-state-switch'
import NetworkStatisticWidgetLogoSourceSelector from './_modules/network-statistic-widget-logo-source-selector'
import NetworkStatisticWidgetSelector from './_modules/network-statistic-widget-selector'
import TrafficRetentionSelector from './_modules/traffic-retention-selector'
import TrayIconConfig from './_modules/tray-icon-config'
import TrayMenuModeSelector from './_modules/tray-menu-mode'
import TrayProxiesSelector from './_modules/tray-proxies-selector'
import WindowCloseBehaviorCards from './_modules/window-close-behavior'

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

const TrafficSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_traffic_label()}</SettingsLabel>

      <SettingsGroup>
        <TrafficRetentionSelector />

        <LocalIpProbeSwitch />
      </SettingsGroup>
    </div>
  )
}

const SystemWidgetSettings = () => {
  const { value: widget } = useSetting('network_statistic_widget')
  const { value: logo } = useSetting('network_statistic_widget_logo')

  // The logo settings only matter while a widget shows.
  const enabled = widget?.kind === 'enabled'

  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>
        {m.settings_nyanpasu_network_statistic_widget_label()}
      </SettingsLabel>

      <SettingsGroup>
        <NetworkStatisticWidgetSelector />

        {enabled && <NetworkStatisticWidgetLogoFollowStateSwitch />}

        {enabled && <NetworkStatisticWidgetLogoSourceSelector />}
      </SettingsGroup>

      {enabled && logo?.source === 'widget' && (
        <NetworkStatisticWidgetIconConfig />
      )}
    </div>
  )
}

const EnhanceSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_enhance_label()}</SettingsLabel>

      <SettingsGroup>
        <BreakWhenProxyChangeSelector />

        <BreakWhenProfileChangeSwitch />

        <BreakWhenModeChangeSwitch />

        <EnableBuiltinEnhancedSwitch />
      </SettingsGroup>
    </div>
  )
}

const TraySettings = () => {
  const { value: trayMenuMode } = useSetting('tray_menu_mode')

  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_tray()}</SettingsLabel>

      <SettingsGroup>
        <TrayMenuModeSelector />

        {trayMenuMode === 'native' && <TrayProxiesSelector />}
      </SettingsGroup>

      {!isMacOS && <TrayIconConfig />}
    </div>
  )
}

const WindowSettings = () => {
  const { value: trayMenuMode } = useSetting('tray_menu_mode')

  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_window_close()}</SettingsLabel>

      <SettingsGroup>
        <WindowCloseBehaviorCards showTrayMenu={trayMenuMode === 'webview'} />
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
      <PageTitle backTo="/main/settings">
        {m.settings_label_nyanpasu()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <LogSettings />

        <TrafficSettings />

        <SystemWidgetSettings />

        <EnhanceSettings />

        <TraySettings />

        <WindowSettings />

        <KeyboardSettings />
      </div>
    </>
  )
}
