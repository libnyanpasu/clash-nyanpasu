import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
import { useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import TrayIconConfig from '../_modules/tray-icon-config'
import { SettingsGroup, SettingsLabel } from '../../../_modules/settings-card'
import TrayMenuModeSelector from './_modules/tray-menu-mode'
import TrayProxiesSelector from './_modules/tray-proxies-selector'
import WindowCloseBehaviorCards from './_modules/window-close-behavior'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu/tray/')({
  component: RouteComponent,
})

function RouteComponent() {
  const showTrayIconConfig = useMatchesPlatform([
    { surface: 'desktop', clientOs: 'windows' },
    { surface: 'desktop', clientOs: 'linux' },
  ])
  const { value: trayMenuMode } = useSetting('tray_menu_mode')

  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_tray()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="tray-settings-container">
          <SettingsLabel>{m.settings_nyanpasu_tray()}</SettingsLabel>

          <SettingsGroup>
            <TrayMenuModeSelector />

            {trayMenuMode === 'native' && <TrayProxiesSelector />}
          </SettingsGroup>

          {showTrayIconConfig && <TrayIconConfig />}
        </div>

        <div data-slot="window-settings-container">
          <SettingsLabel>{m.settings_nyanpasu_window_close()}</SettingsLabel>

          <SettingsGroup>
            <WindowCloseBehaviorCards
              showTrayMenu={trayMenuMode === 'webview'}
            />
          </SettingsGroup>
        </div>
      </div>
    </>
  )
}
