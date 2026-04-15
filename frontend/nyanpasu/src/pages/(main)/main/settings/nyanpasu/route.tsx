import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import HotkeyManager from './_modules/hotket-manager'
import LogFileConfig from './_modules/log-file-config'
import LogLevelSelector from './_modules/log-level-selector'
import TrayIconConfig from './_modules/tray-icon-config'
import TrayProxiesSelector from './_modules/tray-proxies-selector'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
})

const LogSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_logs()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <LogLevelSelector />
          </SettingsCardContent>
        </SettingsCard>

        <LogFileConfig />
      </SettingsGroup>
    </div>
  )
}

const TraySettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_tray()}</SettingsLabel>

      <SettingsGroup>
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

        <TraySettings />

        <KeyboardSettings />
      </div>
    </>
  )
}
